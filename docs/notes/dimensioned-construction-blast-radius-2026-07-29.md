# Dimensioned-construction blast-radius ledger (task 5756, α)

Measurement-only research record. Produces the per-site migration ledger that
`β` (corpus migration), `γ` (predicate promotion), `δ₁` (param/let default
tolerance removal), `δ₂` (constraint-def numeric leniency removal) and the
§6.4 quantity-slot follow-up all cite. **α lands nothing in `crates/`,
`examples/`, `gui/`, or `stdlib`** — every count below is produced by a local,
uncommitted predicate flip that is measured and then fully reverted (proof in
§11(B)). File:line anchors are point-in-time — re-verify against current
`main` before building on one, exactly as `docs/notes/units-gating-gap-research-2026-07-28.md`
warns for its own anchors.

## Provenance

- **Measured at HEAD:** `08c6c42be97d39772f93ee3368c6466bda37fbc6` (short `08c6c42be9`), branch `task/5756`, base `main`.
- **Date:** 2026-07-29.
- **PRD:** `docs/prds/v0_6/dimensioned-construction-strictness.md`, primarily §6.5 ("What α must still measure") and §11 (decomposition plan, α's charter).
- **Task:** 5756 — "dimensioned-construction α: blast-radius measurement + per-site migration ledger (flip the predicate locally, never land it)". Dependency 5753 (ζ₀, the ruling/reversal-history doc) landed as `767c004fd5`.
- **Sibling record:** `docs/notes/units-gating-gap-research-2026-07-28.md` — the same units-strictness program's phase-1/phase-2 research note (HEAD `d2651bce16` / `1195020471`); its "PHASE 2" section independently pre-surveys this same predicate and is cited throughout below where it corroborates or predates a measurement here.

## How to reproduce

**1. The local predicate flip** (never committed — applied, measured, reverted within a single working-tree session; see §11(B) for the revert proof):

```diff
--- a/crates/reify-compiler/src/conformance/mod.rs
+++ b/crates/reify-compiler/src/conformance/mod.rs
@@ -1691,7 +1691,7 @@ fn general_leaf_param_family_is_validated(param_type: &Type) -> bool {
     match param_type {
         Type::Bool | Type::Int | Type::String => true,
-        Type::Scalar { dimension } => dimension.is_dimensionless(),
+        Type::Scalar { .. } => true,
         _ => false,
     }
 }
```

**2. Cargo invocations.** This repo's `cargo` is intercepted by a PreToolUse
condensation wrapper ("skim") that rewrites `cargo test`/`cargo build` output
to terse `PASS: N | FAIL: M | SKIP: K` summaries — useless for harvesting a
per-violation panic transcript. Every invocation below defeats it by
redirecting to a file and reading the file back (`bash -c '... 2>&1' >
/tmp/out.txt 2>&1; cat /tmp/out.txt`), per the project's own documented
bypass list (CLAUDE.md → Mem0 procedural memory on the cargo/skim wrapper).
Exact commands are given in §1 below, next to the transcript they produced.

## Re-verified anchor table

Every anchor the PRD and the task's decompose-time addenda name, re-read
directly against HEAD `08c6c42be9` in this session (not trusted from the PRD
text). All CONFIRMED unless noted.

| # | Anchor | Cited location | HEAD status |
|---|---|---|---|
| 1 | `general_leaf_param_family_is_validated` (the predicate) | `crates/reify-compiler/src/conformance/mod.rs:1691-1697` | **CONFIRMED** — exactly the four arms: `Bool\|Int\|String => true`, `Scalar{dimension} => dimension.is_dimensionless()`, `_ => false`. |
| 2 | Gate 2 (conformance param-default entry) | `conformance/mod.rs:517-532` | **CONFIRMED** — `_ =>` arm; anti-cascade `Type::Error` guard at :519-521; `severity: CTOR_FIELD_CONFORMANCE_SEVERITY` at :530; `walk_param_against_arg(&vc.cell_type, default, &mut ctx)` at :532. |
| 3 | `CTOR_FIELD_CONFORMANCE_SEVERITY` | `conformance/mod.rs:32` | **CONFIRMED** — `Severity::Warning`. α does not touch this const. |
| 4 | Walker lockstep recursion | `conformance/mod.rs:669-701` | **CONFIRMED** — `walk_param_against_arg`: `Option`/`List`(+`ReflectiveCellList`)/`Set`/`Map` arms recurse; `TraitObject` is a leaf (handled after :704). |
| 5 | Gate 5 / Rule 4 (constraint-def numeric leniency) | `crates/reify-compiler/src/type_compat.rs:1482-1486` | **CONFIRMED** — `let is_numeric = |t| matches!(t, Type::Int \| Type::Scalar{..} \| Type::ScalarParam(_)); if is_numeric(param_ty) && is_numeric(arg_ty) { return true }`. Rule 2 at :1474 (`type_carries_type_param \|\| type_carries_trait_object`) already short-circuits before Rule 4. |
| 6 | Fn-call entry guard (i) | `compile_builder/entities_phase.rs:1493` | **CONFIRMED** — `if !type_carries_trait_object(param_ty) { continue; }`, immediately preceding the only production `check_fn_arg_conformance` call at `:1503`. |
| 7 | `OverloadResolution::Resolved` gate | `compile_builder/entities_phase.rs:1486-1488` | **CONFIRMED, and this is anchor drift versus the PRD.** The PRD's own text cites `:1508-1511`; the real anchor at HEAD is `:1486-1488` (`let f = match resolve_function_overload(...) { OverloadResolution::Resolved(f) => f, _ => return };`). Addendum C1's correction re-confirmed. |
| 8 | Guard (ii) candidate backstop, `resolve_function_overload`'s filter | `type_compat.rs:1155` (fn), filter body `:1194-1201` | **CONFIRMED, and FALSIFIED as an independent backstop.** The filter's first disjunct is `type_carries_trait_object(param_ty)` (`:1196`) — the **same predicate** as guard (i). The two guards are mutually exclusive, not conjunctive. See §7 (item 4) for the reachability consequence. |
| 9 | Corpus gate test | `crates/reify-compiler/tests/examples_smoke.rs:198` (`no_example_emits_ctor_field_conformance_diagnostics`) | **CONFIRMED** — `exercised >= 40` floor at :215-220; per-violation panic body (`file [code] message`) at :222-241; `skip_set_entries_exist_under_examples_dir` sanity guard at :248; walks `examples/` only via `discover_ri_files`/`EXAMPLES_DIR` (not the wider `corpus_no_bare_scalar.rs` tree). |
| 10 | `NAMED_DIMENSIONS` registry | `crates/reify-core/src/dimension.rs:514` | **CONFIRMED present; cardinality corrected — see §0.** The array itself is unchanged in shape from what the PRD describes; the number of entries needed independent re-verification (three prior figures disagreed: 49/51/34). |
| 11 | `corpus_no_bare_scalar.rs` reuse target | `crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs` | **CONFIRMED, reused for infrastructure only — see caveat in §0.** `collect_files` at :46; five-tree walk at :143-160; self-exclusion at :170; parse-only exclusion of `reify-syntax/tests` + `reify-ast/tests` at :183-186. Its own `line_has_bare_scalar` predicate (matching the literal keyword `Scalar` bare) is **not** reused — it answers a different, already-closed migration (bare `Scalar` keyword → named dimension). α reuses the walk/exclusion machinery and comment-stripping technique, and writes its own predicate for bare *numeric literals* at dimensioned-type positions. |

---

## §0 Measurement substrate — dimension-name set (pre-2)

Everything downstream (§§2-9) is a grep-shaped sweep keyed on a **name set**:
every identifier that can appear at a dimensioned-Scalar type position in
`.ri` source. Two components, unioned; both re-measured mechanically at HEAD
`08c6c42be9`, never taken from the PRD's prose figures.

### 0.1 `NAMED_DIMENSIONS` cardinality — the 49/51/34 discrepancy, resolved

Before this task started, **three different cardinalities for the same
static array were live in the repo**, none agreeing:

| Source | Figure | Status after re-measurement |
|---|---|---|
| `dimension.rs:512-513`'s own doc comment ("The slice contains 34 entries") | 34 | **STALE — contradicted.** Pre-existing bug in the source doc comment, out of scope to fix here (α lands nothing in `crates/`); worth a follow-up. |
| PRD §6.2 methodology paragraph | 51 names | **CONFIRMED accurate at current HEAD** (see below) — not stale, despite this task's own decompose-time analysis assuming it was. |
| This task's plan `analysis` / prerequisite-2 description (decompose-time) | 49 entries "counted mechanically" | **NOT REPRODUCIBLE by a direct HEAD re-count** — see mechanism below. Reported non-blocking via `escalate_info` (`esc-5756-1`) before writing this section. |

**The HEAD-measured figure, re-derived three independent ways in this
session, is 51:**

1. Manual enumeration — reading `crates/reify-core/src/dimension.rs:514-595`
   (the full `NAMED_DIMENSIONS` array literal) end to end and listing every
   `(DimensionVector::X, "Name")` tuple by hand: 51 tuples, `Length` .. `Momentum`.
2. `grep -o 'DimensionVector::' ` restricted to the array's line range (514-595)
   returns 51 matches; manually auditing every matching line confirms each is
   a genuine tuple constructor, not a comment (the array's surrounding
   comments discuss dimension names in prose — e.g. "`TranslationalStiffness`
   is dimensionally identical to `STIFFNESS`" — but never spell the substring
   `DimensionVector::` inside a comment in this range).
3. A corrected extraction script (comment-lines stripped, tuple regex
   allowing whitespace after `(`) lists the same 51 names as method 1,
   verbatim and in the same order (recorded in full below).

**Root cause of the "49" figure (mechanism reproduced live in this
session):** a plain `\(DimensionVector::` regex — i.e. one that requires the
tuple's content to immediately follow the opening paren with no
intervening whitespace — silently drops exactly the two entries whose
identifier names are long enough that rustfmt wraps their tuple across 4
lines instead of 1 (`MagneticFluxDensity` at :539-542, `ElectricalConductivity`
at :558-561: `(\n    DimensionVector::MAGNETIC_FLUX_DENSITY,\n    "MagneticFluxDensity",\n),`).
51 − 2 = 49, matching the observed delta exactly. This session's first
extraction attempt reproduced the identical bug (49, with the same two
names missing) before the regex was fixed to allow leading whitespace — the
mechanism is not hypothetical, it was caught in the act.

**A second, independent, non-erroneous "49" also exists in the ecosystem**
and is a more likely source of the plan's figure: `docs/notes/units-gating-gap-research-2026-07-28.md`
line 308 (a prior, independently-verified research note in this same
program) states *"NAMED_DIMENSIONS doc says 34, actually 51 names/49
vectors (stale)"* — i.e. of the 51 `(vector, name)` tuples, only **49 are
distinct `DimensionVector` *values*** (a handful of names are deliberate
aliases for the same physical dimension — confirmed for at least 3 pairs by
inspection: `Stiffness`/`TranslationalStiffness` share `DimensionVector::STIFFNESS`
by explicit const alias at `dimension.rs:289`; `AbsorptionCoeff`/`Curvature`
both compute `from_exps(&[(0,-1)])` independently and land on the same
value; `Impulse`/`Momentum` literally reuse `DimensionVector::IMPULSE` for
two names at `dimension.rs:593-594`). **"49 distinct vectors" and "51 names"
are both true simultaneously — they answer different questions** (distinct
physical dimensions vs. distinct spellable type names), and this task's
decompose-time analysis most plausibly conflated the two rather than
mis-measuring from scratch.

**RULING (governs every count in §§2-9):** the name set used for the sweep is
**51 names**, because the sweep is a *textual* search over `.ri` type
annotations — what matters is which spellings are legal, not how many
distinct physical dimensions they collapse to. The stale "34" is never used.
The "49 distinct vectors" fact is retained here as an explanatory footnote,
not as a sweep input.

Full 51-name list, mechanically extracted at HEAD `08c6c42be9`
(`crates/reify-core/src/dimension.rs:514-595`), in source order:

```
Length, Mass, Time, Current, Temperature, AmountOfSubstance, LuminousIntensity,
Angle, SolidAngle, Money, Area, Volume, Frequency, Force, Energy, Power,
Pressure, Voltage, Charge, Capacitance, Resistance, Conductance, Inductance,
MagneticFlux, MagneticFluxDensity, LuminousFlux, Illuminance, AbsorbedDose,
AngularVelocity, DynamicViscosity, MomentOfInertia, Density, Velocity,
Acceleration, ForceDensity, ThermalConductivity, SpecificHeat,
ThermalExpansion, ElectricResistivity, ElectricalConductivity,
DielectricStrength, Stiffness, TranslationalStiffness, RotationalStiffness,
RotationalDamping, TranslationalDamping, AbsorptionCoeff, Curvature,
FractureToughness, Impulse, Momentum
```

Reproduction command:

```bash
awk '/^pub static NAMED_DIMENSIONS/,/^\];/' crates/reify-core/src/dimension.rs \
  | grep -v '^\s*//' | tr -d '\n' \
  | grep -oE '\(\s*DimensionVector::[A-Z_0-9]+,\s*"[A-Za-z]+"\s*,?\s*\)' \
  | grep -oE '"[A-Za-z]+"' | tr -d '"'
```

### 0.2 The `.ri` type-alias extension (§6.2's mandatory union)

A registry-only sweep under-counts: several dimensioned Scalar types are
spelled as `.ri`-level `type`/`pub type` aliases over dimension *arithmetic*
(`Force * Length / Angle`, etc.) and never appear in `NAMED_DIMENSIONS` at
all. Enumerated by grepping every tracked `.ri` file for `(pub )?type
<Name>(<...>)? = <rhs>` (595 tracked `.ri` files; 37 raw hits) and manually
classifying each:

| Name | Definition site | RHS | Verdict |
|---|---|---|---|
| `Stress` | `crates/reify-compiler/stdlib/analysis.ri:13` | `= Pressure` | **NEW — dimensioned.** |
| `Torque` | `crates/reify-compiler/stdlib/ports_mechanical.ri:29` | `= Force * Length / Angle` | **NEW — dimensioned.** |
| `HeatFlux` | `crates/reify-compiler/stdlib/ports_thermal.ri:34` | `= Power / Area` | **NEW — dimensioned.** |
| `ThermalResistance` | `crates/reify-compiler/stdlib/ports_thermal.ri:39` | `= Temperature / Power` | **NEW — dimensioned.** |
| `ArealCostRate` | `crates/reify-compiler/stdlib/surface_finish.ri:32` (also redeclared in two out-of-corpus-reach PRD fixtures, `docs/prds/v0_6/fixtures/surface_finish_{area_cost,functional}.ri`) | `= Money / Area` | **NEW — dimensioned.** |
| `HeatCapacity` | `crates/reify-compiler/stdlib/units.ri:110` | `= Energy / Temperature` | **NEW — dimensioned.** |
| `InverseAmount` | `crates/reify-compiler/stdlib/units.ri:116` | `= Dimensionless / AmountOfSubstance` | **NEW — dimensioned** (exponent on `AmountOfSubstance` is −1 ≠ 0). |
| `Action` | `crates/reify-compiler/stdlib/units.ri:120` | `= Energy * Time` | **NEW — dimensioned.** |
| `StefanBoltzmannDim` | `crates/reify-compiler/stdlib/units.ri:127` | `= Power / Area / Temperature^4` | **NEW — dimensioned.** |
| `Permittivity` | `crates/reify-compiler/stdlib/units.ri:131` | `= Capacitance / Length` | **NEW — dimensioned.** |
| `Permeability` | `crates/reify-compiler/stdlib/units.ri:135` | `= Inductance / Length` | **NEW — dimensioned.** |
| `MolarGasConstant` | `crates/reify-compiler/stdlib/units.ri:139` | `= Energy / AmountOfSubstance / Temperature` | **NEW — dimensioned.** |
| `VolumetricFlowRate` | `crates/reify-compiler/stdlib/ports_fluid.ri:34` | `= Volume / Time` | **NEW — dimensioned.** |
| `Jerk` | `examples/integration_corner_cases.ri:25` | `= Acceleration / Time` | **NEW — dimensioned, but FILE-LOCAL** (declared without `pub`, inside one example file — see caveat below). The plan's decompose-time analysis named this one by guess without a confirmed anchor; confirmed present here. |
| `Velocity` | `crates/reify-compiler/stdlib/units.ri:96` | `= Length / Time` | Redundant — same name already in `NAMED_DIMENSIONS` (task 4580 registered it there; this alias is dead code per its own doc comment at units.ri:87-95). Not a new name. |
| `JointValue` | `crates/reify-compiler/stdlib/trajectory.ri:77` | `= Real` | Excluded — dimensionless (`Real` = dimensionless `Scalar`), already in the vetted family today. |
| `Strain` | `crates/reify-compiler/stdlib/analysis.ri:19` | `= Dimensionless` | Excluded — dimensionless, same reason. |
| `Pose3` | `crates/reify-compiler/stdlib/trajectory.ri:91` | `= Transform3` | Excluded — not a Scalar. |
| `LocationId` | `crates/reify-compiler/stdlib/trajectory.ri:119` | `= FaceSelector` | Excluded — not a Scalar. |
| `Vec3<Q: Dimension>` | `crates/reify-compiler/stdlib/trajectory.ri:103` | `= Vector3<Q>` | Excluded — generic vector wrapper, §6.4/item-5 territory, not a flat Scalar leaf. |
| `Rate<Q: Dimension>` | `crates/reify-compiler/stdlib/units.ri:106` | `= Q / Time` | Excluded from the flat name list — generic. **Methodology gap, not a count change:** a concrete instantiation like `Rate<Length>` *does* resolve to a genuine dimensioned `Type::Scalar(VELOCITY)` (confirmed live: `crates/reify-compiler/tests/cross_module_alias_propagation_tests.rs:462`, `param v : Rate<Length>`), and is textually invisible to a grep keyed on the flat 51+13 name list (the text says `Rate<Length>`, not a bare `Length`). Spot-checked the one live corpus instance — it declares no default (`param v : Rate<Length>` only), so it cannot be a bare-literal violation today, but a *future* `param v : Rate<Length> = 5` site would be a real blind spot this sweep cannot see. Recorded here rather than silently passed over. |
| `Vel<Q: Dimension>` (`= Q / Time`), `Wrap<U: Dimension>` (`= Box2<U>`) | `crates/reify-compiler/tests/fixtures/parametric_alias_def_site_ok.ri:11,23` | — | Excluded — generic, Rust-test-fixture-only. |
| `LeakName<Q: Dimension>`, `BadBound<P>` | `crates/reify-compiler/tests/fixtures/parametric_alias_def_site_reject.ri:10,21` | — | Excluded — deliberately-invalid negative-test fixtures (filename says `_reject`); never resolve. |
| `Acceleration`, `Pressure` (local redeclarations) | `examples/integration_corner_cases.ri:23-24` (Velocity/Acceleration, part of the same 3-deep chain as Jerk), `examples/integration_full_v01.ri:28`, `prj/printer_v01/printer.ri:60` | — | Redundant — same names already in `NAMED_DIMENSIONS`, locally re-derived to the same dimension value (demonstration/self-contained-fixture purposes). Not new names. `prj/**` is additionally outside the `corpus_no_bare_scalar.rs` walk's reach (§11(A)). |
| `Material` (×3), `MotionValue` (×3) | `examples/trait_assoc_type_{material,qualified}.ri`, `tree-sitter-reify/test/fixtures/trait_assoc_type_bind.ri`, `crates/reify-compiler/stdlib/kinematic.ri:133,151,198` | — | Excluded — **not the same mechanism.** These are trait **associated-type bindings** (`structure def X : Trait { type AssocName = Concrete }`), syntactically similar but semantically different from a free-standing `pub type Name = expr` alias. `MotionValue` is `HasMotion`'s associated type, bound to `Length` for `Prismatic` (kinematic.ri:133) and `Angle` for `Revolute` (kinematic.ri:151) — both already-named dimensions, so no new name either way. Flagged so a later reader does not conflate the two mechanisms. |

**Net addition: 14 new names** (`Stress`, `Torque`, `HeatFlux`,
`ThermalResistance`, `ArealCostRate`, `HeatCapacity`, `InverseAmount`,
`Action`, `StefanBoltzmannDim`, `Permittivity`, `Permeability`,
`MolarGasConstant`, `VolumetricFlowRate`, `Jerk`), zero collisions with the
51-name registry set (verified with `comm -12`).

### 0.3 Final search set

**51 (registry) + 14 (alias union) = 65 names.** This is the name set every
grep in §§2-9 is keyed on. Saved at
`/tmp/5756-scratch/full-dimension-name-set.txt` (scratch, uncommitted, not
part of this task's diff — see §11(A) for the reach/gate discussion and
§11(B) for the no-source-change proof).

### 0.4 Tree-walk substrate (reused, not reinvented)

Per the reuse item in the plan, the walk is lifted from
`crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs` — its five
trees, its self-exclusion, and its parse-only exclusion — but driven from
`git ls-files` (not a raw filesystem walk) specifically so stale
`.claude/worktrees/` / `.eval-worktrees/` copies cannot leak in and inflate
counts (`corpus_no_bare_scalar.rs` itself doesn't have this problem — `Path`
walks from `workspace_root()` inside a single worktree — but a *scratch*
re-walk for this task easily would, since `.claude/worktrees/` under a live
repo checkout is common and holds full nested copies of the same trees).
Its own `line_has_bare_scalar` predicate is **not** reused (§0 anchor table
note 11) — it detects the already-migrated bare `Scalar` *keyword*, a
different, closed migration.

Measured tree sizes (`git ls-files`, HEAD `08c6c42be9`):

| Tree | Pattern | Count |
|---|---|---|
| A | `examples/*.ri` (recursive) | 258 |
| B | `crates/*.ri` (recursive) | 168 |
| C | `crates/*.rs` (recursive), minus self (`corpus_no_bare_scalar.rs`) and the two parse-only dirs (`crates/reify-syntax/tests/`, `crates/reify-ast/tests/`) | 1708 raw → **1641** after exclusion |
| D | `gui/src-tauri/*.rs` (recursive) | 41 |
| E | `gui/test/*.ri` (recursive) | 12 |
| **Union (sorted, deduped)** | | **2120** |

Reproduction:

```bash
git ls-files -- 'examples/*.ri' 'crates/*.ri' 'crates/*.rs' \
                'gui/src-tauri/*.rs' 'gui/test/*.ri' \
  | grep -v 'corpus_no_bare_scalar\.rs$' \
  | grep -v '^crates/reify-syntax/tests/\|^crates/reify-ast/tests/' \
  | sort -u
```

## §1 Flipped-predicate run + transcript (step-1)

The local flip from the repro block was applied, all four suites named by
the plan were run and their full (skim-wrapper-bypassed) output captured,
then the flip was reverted and `git diff --exit-code -- crates/` re-confirmed
clean before this section was written. Build environment: `RUSTC_WRAPPER=sccache
CARGO_INCREMENTAL=0` (per CLAUDE.md), warm seeded `target/`.

### 1.1 Summary

| Suite | Crate | Result under flip | Detail |
|---|---|---|---|
| `examples_smoke::no_example_emits_ctor_field_conformance_diagnostics` | `reify-compiler` | **RED** (exit 101) | 7 diagnostics / 250 exercised `examples/**` files |
| `struct_ctor_field_conformance_tests` (whole file, incl. `param_default_*`) | `reify-compiler` | **RED** (exit 101) | 39 passed, 1 failed — exactly one test, a deliberate negative fixture |
| `input_shape_eval_e2e` | `reify-eval` | GREEN (exit 0) | 3/3 passed — **uninformative**, see 1.4 |
| `auto_type_param_determinism_tests` | `reify-eval` | GREEN (exit 0) | 11/11 passed — **uninformative**, see 1.4 |

### 1.2 `examples_smoke` transcript (verbatim, compile noise elided)

Command: `RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0 cargo test -p reify-compiler --test examples_smoke no_example_emits_ctor_field_conformance_diagnostics -- --nocapture --test-threads=1`

```
running 1 test
test no_example_emits_ctor_field_conformance_diagnostics ...
thread 'no_example_emits_ctor_field_conformance_diagnostics' (1643844) panicked at crates/reify-compiler/tests/examples_smoke.rs:228:9:
ctor-conformance corpus gate: 7 diagnostic(s) across 250 exercised example files.
Every one is a struct-ctor field-conformance diagnostic fired against a shipped example — i.e. a false positive from the conformance walker, not a broken example.
Fix the walker in crates/reify-compiler/src/conformance/mod.rs — either the family's dedicated shape-based arm in `walk_param_against_arg_type` (Vector / Point / Field / Matrix / Tensor) or the `general_leaf_param_family_is_validated` allowlist that gates the general concrete-leaf arm; do NOT add a SKIP_SET entry.

  bearing_auto_seal.ri [ArgTypeMismatch] argument 'durometer' has type 'Real' but param 'durometer' requires type 'Scalar[m]'
  trajectory/printer_print_envelope.ri [ArgTypeMismatch] argument 'velocity_limit' has type 'Real' but param 'velocity_limit' requires type 'Scalar[m·s^-1]'
  trajectory/printer_print_envelope.ri [ArgTypeMismatch] argument 'acceleration_limit' has type 'Real' but param 'acceleration_limit' requires type 'Scalar[m·s^-2]'
  trajectory/printer_print_envelope.ri [ArgTypeMismatch] argument 'max_force' has type 'Real' but param 'max_force' requires type 'Scalar[m·kg·s^-2]'
  trajectory/tots_optimal_ptp.ri [ArgTypeMismatch] argument 'max_force' has type 'Real' but param 'max_force' requires type 'Scalar[m·kg·s^-2]'
  trajectory/tots_optimal_ptp.ri [ArgTypeMismatch] argument 'velocity_limit' has type 'Real' but param 'velocity_limit' requires type 'Scalar[m·s^-1]'
  trajectory/tots_optimal_ptp.ri [ArgTypeMismatch] argument 'acceleration_limit' has type 'Real' but param 'acceleration_limit' requires type 'Scalar[m·s^-2]'
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
FAILED

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 5 filtered out; finished in 4.75s
```

`exercised = 250` = 258 tracked `examples/**/*.ri` files minus 8 `SKIP_SET`
entries (`examples_smoke.rs:26-122`) — reconciles exactly.

**Reconciliation against the already-known §6.2/§6.3 tables (cross-check
required by step-3):** these 7 hits are the **union** of two already-documented
buckets, with zero new sites:
- **6 are §6.3 BARE ctor-call sites**, already in the known 17-site table:
  `printer_print_envelope.ri` ×3 (`velocity_limit`, `acceleration_limit`,
  `max_force`) + `tots_optimal_ptp.ri` ×3 (same three fields) — gate 1.
- **1 is the §6.2 category-A param-default site**: `bearing_auto_seal.ri`'s
  `durometer` — gate 2, fired against the structure's own default value
  (`param durometer : Length = 70.0`), not a call site (no `NitrileSeal(durometer: …)`
  call exists anywhere in the corpus — confirmed by grep). The diagnostic
  wording (`argument 'durometer' has type 'Real' but param 'durometer'
  requires type 'Scalar[m]'`) is identical in shape to a ctor-call violation
  because both gates share the same `walk_param_against_arg` machinery
  (§0 anchor table #2, #4) — only the call site (default-expression vs.
  named ctor arg) distinguishes them.

6 + 1 = 7. **No transcript hit falls outside the known table — the
cross-check is clean.**

### 1.3 `struct_ctor_field_conformance_tests` transcript (verbatim, passing tests elided)

Command: `RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0 cargo test -p reify-compiler --test struct_ctor_field_conformance_tests -- --test-threads=4`

```
running 40 tests
test excluded_family_dimensioned_scalar_given_dimensionless_real_is_silent ... FAILED

failures:

---- excluded_family_dimensioned_scalar_given_dimensionless_real_is_silent stdout ----

thread 'excluded_family_dimensioned_scalar_given_dimensionless_real_is_silent' (1680670) panicked at crates/reify-compiler/tests/struct_ctor_field_conformance_tests.rs:1455:5:
dimensioned Scalar params given dimensionless Real args must emit ZERO ctor-conformance diagnostics — a bare numeric literal at a dimensioned slot is idiomatic across the corpus. Got: [
    Diagnostic {
        severity: Warning,
        message: "argument 'velocity_limit' has type 'Real' but param 'velocity_limit' requires type 'Scalar[m·s^-1]'",
        ...
        code: Some(ArgTypeMismatch),
    },
    Diagnostic {
        severity: Warning,
        message: "argument 'acceleration_limit' has type 'Real' but param 'acceleration_limit' requires type 'Scalar[m·s^-2]'",
        ...
        code: Some(ArgTypeMismatch),
    },
]

failures:
    excluded_family_dimensioned_scalar_given_dimensionless_real_is_silent

test result: FAILED. 39 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.70s
```

**Classification: DELIBERATE NEGATIVE TEST — TO INVERT.** The test's own name
(`..._is_silent`) and assertion message ("must emit ZERO ctor-conformance
diagnostics — a bare numeric literal at a dimensioned slot is idiomatic
across the corpus") assert exactly the pre-flip behaviour this whole PRD
exists to reverse; `γ` inverts this test's expectation (or removes it) as
part of promoting the predicate. This is the canonical example of the
"DELIBERATE-NEGATIVE-TEST-TO-INVERT" disposition used throughout §§2-9.
Full diagnostic bodies (elided above with `...`) are byte-identical to the
`printer_print_envelope.ri` pair already quoted in §1.2 — same underlying
fixture data, exercised via a second harness path.

### 1.4 Why the two `reify-eval` suites stayed GREEN (structural finding, not a data point)

Both `input_shape_eval_e2e.rs` and `auto_type_param_determinism_tests.rs`
passed 100% under the flip (3/3 and 11/11). This is **not evidence their
fixtures are clean** — `input_shape_eval_e2e.rs:248,255,256` are three of
the already-known 17 BARE sites (`JointLimit(joint: 0.0, max_force: 100.0)`,
`TOTSShaper(..., velocity_limit: 300.0, acceleration_limit: 5000.0, ...)`)
and still fire Warning-severity diagnostics under the flip. Grepping both
files for `diagnostics` confirms neither suite ever inspects
`compiled.diagnostics` for emptiness — `input_shape_eval_e2e.rs`'s own doc
comment says it "panics with diagnostics on any **compile error**" only,
i.e. it only reacts to `Severity::Error`. Since `CTOR_FIELD_CONFORMANCE_SEVERITY`
is `Warning` (§0 anchor table #3, untouched by α), compilation still
succeeds and these suites' actual assertions (evaluated numeric results,
determinism/candidate-selection behaviour) are unaffected. **General rule
confirmed by this session's four-suite sample: a suite only goes RED under
the flip if it explicitly asserts on ctor-conformance diagnostic content
(`examples_smoke`'s corpus-wide scan, or a dedicated `struct_ctor_field_conformance_tests`
assertion) — compile-success-only suites are silent regardless of how many
BARE sites they touch.** This matters for reading §§2-9: RED/GREEN test
status is not a substitute for the direct-inspection site classification
those sections perform.

### 1.5 Revert proof

```
$ git diff --exit-code -- crates/
$ echo $?
0
```

No diagnostic/behaviour change survives this section; the predicate at
`conformance/mod.rs:1694` reads `Type::Scalar { dimension } => dimension.is_dimensionless(),`
identically to HEAD both before and after this measurement.

## §2 Item 7a — §6.2 re-confirmation: gates 3+4, δ₁'s blast radius (step-2)

Re-confirms §6.2's table (categories A/B/C, the two `.ri` sites, and the
EXCLUDED-BY-DESIGN `pub unit` bucket) against HEAD, using pre-2's
alias-extended 65-name set and the reused tree walk. **Method, not just
figures, is reproduced below** because §6.2 gives no per-site table for
category C (unlike §6.3) — re-confirming an aggregate-only count forces an
independent site-level derivation, which is exactly what this section is.

### 2.1 Category A/B — `.ri` bare-literal param/let defaults

**Category A (`param`) = 2. CONFIRMED, unchanged from §6.2.**

```bash
ALT=$(cat /tmp/5756-scratch/names-alternation.txt)   # 65-name set, §0.3
for f in $(cat tree-A-examples-ri.txt tree-B-crates-ri.txt tree-E-gui-test-ri.txt); do
  grep -nP "^\s*(priv\s+)?(param|let)\s+[A-Za-z_][A-Za-z0-9_]*\s*:\s*($ALT)\s*=\s*-?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?\s*[,)]?\s*(//.*)?$" "$f"
done
```

| File:line | Text | In sweep's reach? |
|---|---|---|
| `examples/bearing_auto_seal.ri:46` | `param durometer : Length = 70.0` | YES — found by the strict regex over trees A/B/E |
| `tree-sitter-reify/test/fixtures/mv-2-priv-param.ri:4` | `priv param rated_torque : Torque = 5` | **NO** — this tree is outside `corpus_no_bare_scalar.rs`'s walk (§11(A)); confirmed present by direct `sed` read, not by the sweep |

Both verbatim-confirmed at HEAD. **Only the first is type-checked** —
`mv-2-priv-param.ri` is a `tree-sitter-reify` grammar fixture, parsed by the
tree-sitter CLI/grammar tests only, never reaches `reify-compiler`.

**Category B (`let`) = 0. CONFIRMED.** Widened the query to list **every**
`let <n> : <DIMENSIONED> = …` site regardless of literal-vs-expression shape
(35 sites, trees A/B) to sanity-check the regex wasn't just missing bare
forms: all 35 are compound expressions (`.sum`, `cost(...)`, `auto`,
`auto(free)`, `unwrap_or(...)`, `match …`, `Defaultable::scaled(3.0)` — the
`3.0` there is a **call argument**, not the let's own default). Zero are a
bare literal. Full list in `/tmp/5756-scratch/step2-let-dimensioned-any.txt`.

### 2.2 Category C — Rust fixture-string occurrences

**PRD figure: 63 across 22 files (25 c1 + 10 c2 + 28 c0, 11 of the 28
parse-only). My re-measurement: 55 in-scope hits across 17 files (15 c1 + 10
c2 + 26 c0 + 2 not conclusively classified within this session's budget).**
Recorded as a DELTA, not silently reconciled — see 2.2.4.

**2.2.1 Method.** A line-level scan of every file in tree C
(`crates/**/*.rs`, 1641 files after exclusions) + tree D
(`gui/src-tauri/**/*.rs`, 41 files), reusing `corpus_no_bare_scalar.rs`'s
comment-stripping technique (skip lines whose trimmed start is `//`; strip
a trailing `// …` unless preceded by `:`), against a purpose-written bare
predicate (this task's C2 anchor note: `corpus_no_bare_scalar.rs`'s own
`line_has_bare_scalar` detects the bare `Scalar` *keyword*, a different,
closed migration — reusing it here would answer the wrong question):

```python
pat = re.compile(
    r'(?:priv\s+)?(?:param|let)\s+[A-Za-z_][A-Za-z0-9_]*\s*:\s*(?:' + alt65 + r')\b'
    r'\s*=\s*-?[0-9]+(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?(?![a-zA-Z_0-9.])'
    r'(?=\s*(?:,|\)|;|\}|//|"|\\|$))'
)
```

**2.2.2 Two regex bugs found and fixed while building this — recorded
because a naive version of this sweep silently over-counts.** (1) A
negative lookahead of just `(?![a-zA-Z_0-9])` after the number lets the
engine backtrack the optional fractional group and accept the *integer
prefix* of a unit-suffixed literal (`0.2mm` → backtracks to bare `0`,
followed by `.`, which isn't excluded) — fixed by adding `.` to the
lookahead. This bug alone produced 122 false positives (170 raw hits → 48
after the fix), overwhelmingly unit-suffixed (`0.2mm`, `0.50USD`) or
dimensionally-explicit compound expressions (`30.0 * 1W / (1m * 1K)`) — both
of which are the PRD's *other* two rows (818 unit-suffixed / 68 compound),
not category C. (2) The end-of-match assertion must allow a following `}`,
`"`, or a literal backslash (a Rust string's embedded `\n` escape, two
characters, when a whole `.ri` snippet sits on one physical `.rs` line) —
without it, one-line fixtures like `"structure S {\n  param x: Length = 1\n}"`
are missed. Both fixes verified by direct inspection of the dropped/added
lines (`/tmp/5756-scratch/dropped-keys.txt`).

**2.2.3 Two sites excluded as a different mechanism, not counted in the 55.**
`let_type_disambiguation_tests.rs:296` (`trait HasX { let x : Length = 5.0 }`)
and the already-known `m9_error_cases.rs:276` (`let score : Mass = 1.5`,
cited by §6.2 itself) both place the bare literal inside a **trait
requirement `let`**, checked by `checker.rs`'s injected-default cross-check
— a structurally different, already-strict path from gates 3/4
(`entity.rs:479-485`/`:563-569`, which govern **structure-body**
`param`/`let` declarations only). Trait-lets being already-strict is §6.2's
own finding; my regex doesn't distinguish structure-body from
trait-requirement position, so these two are a methodology false-positive,
not a new site.

**2.2.4 c1/c2/c0 classification.** Classified each remaining hit by reading
its enclosing test function and asking: *does this test's assertion
exhaustively check for absence of `Severity::Error` (`errors_only(...)`,
`error_diags(...)`, `.filter(severity==Error).collect()` then
`assert!(...is_empty())`, or call the panicking helper `parse_and_compile`/
`eval_source`/`check_source`, which assert no compile errors internally),
or does it only check for a *specific* diagnostic (`.find(...)`/`.any(...)`
on message/code content, non-exhaustive)?* Only the former breaks under
δ₁ — a hit can newly produce a diagnostic and NOT break its test, exactly
the general rule §1.4 already established for the flipped-predicate
transcript.

*c2 — deliberate pins to invert (10, exact match to PRD's count and
location):*

| File:line(s) | Test | Sites |
|---|---|---|
| `param_default_type_mismatch_tests.rs:175-178` | `param_int_and_real_literal_on_dimensioned_scalar_do_not_error` | 4 (`zero_int=0, one_int=1, half_real=0.5, large_real=70.0`) |
| `param_default_type_mismatch_tests.rs:206-207` | `param_negative_literal_on_dimensioned_scalar_does_not_error` | 2 (`neg_real=-5.0, neg_int=-1`) |
| `let_annotation_type_mismatch_tests.rs:176-178` | `let_annotation_int_and_real_literal_on_dimensioned_scalar_do_not_error` | 3 (`x=5, y=0.5, z=-5.0`) |
| `let_annotation_type_mismatch_tests.rs:583` | `port_member_let_annotation_numeric_literal_on_dimensioned_scalar_do_not_error` | 1 (`d=5`) |

These are exactly "the 4 tests of §0.3" the PRD names, and the count (10)
matches exactly — strong evidence the rest of this section's methodology is
sound even where the aggregate total drifts from 63.

*c1 — would break (15 CONFIRMED by direct read):*

| File:line | Site | Why it breaks |
|---|---|---|
| `collection_sub_tests.rs:426` | `grade : Length = 8.8` | uses `parse_and_compile` (panics on any `Severity::Error`) |
| `collection_sub_tests.rs:510` | `grade : Length = 8.8` | same |
| `harness_langcore/let_scope_tests.rs:2190` | `axis: Length = 0` | `assert!(errors.is_empty(), …)` exhaustive |
| `harness_langcore/let_scope_tests.rs:2251` | `axis: Length = 0` | same |
| `harness_langcore/let_scope_tests.rs:2359` | `axis: Length = 0` | same |
| `harness_traits/trait_assoc_type_conformance_tests.rs` (override sub-case) | `w : Length = 1` | `errors_only(&module).is_empty()` exhaustive, "override should compile cleanly" |
| `harness_traits/trait_assoc_type_conformance_tests.rs` (inherited-default sub-case) | `w : Length = 1` | same, "inherited default should compile cleanly" |
| `purpose_compile_tests.rs:1453` | `material : Length = 1.0` | `errors.is_empty()` exhaustive |
| `purpose_compile_tests.rs:1454` | `youngs_modulus : Length = 200.0` | same |
| `reify-eval/tests/collection_sub_eval.rs:437` | `grade : Length = 8.8` | manual `.filter(severity==Error)` then `assert!(errors.is_empty())` exhaustive |
| `reify-eval/tests/determinacy_predicates.rs:509` | `a : Length = 10` | via `eval_source(...)` → internally `parse_and_compile`, panics on any Error |
| `reify-eval/tests/purpose_activation.rs:2673` | `material : Length = 100.0` | `parse_and_compile`; doc comment itself notes "panics on any Severity::Error" |
| `reify-eval/tests/purpose_activation.rs:2674` | `youngs_modulus : Length = 200.0` | same |
| `reify-eval/tests/purpose_activation.rs:2774` | `z : Length = 5.0` | same |
| `reify-eval/tests/purpose_activation.rs:2825` | `z : Length = -5.0` | same |

*c0 — no break (26 confirmed/high-confidence by direct read):*

| File:line(s) | Reason |
|---|---|
| `let_scope_tests.rs:2025,2471,2540` (3) | `.find(...)`/`.any(...)` on specific message content, non-exhaustive |
| `reify-syntax/src/ts_parser.rs:6338,6364,6420,6450` (4, one line double-counts to 2 sites) | **structurally parse-only** — `reify-syntax`'s normal deps exclude `reify-compiler`; these tests call the crate's own `parse()`, never a compiler/conformance path |
| `reify-test-support/src/helpers.rs:1077,1078,1227` (3) | `1227` confirmed (checks `!result.values.is_empty()`, never inspects diagnostics); `1077/1078` presumed (compile_template's return isn't asserted clean in the slice read) |
| `prelude_context_tests.rs:217` (1) | asserts **relative parity** between two compile entry points on the *same* source, not absolute cleanliness — a new diagnostic would land identically on both sides and parity would still hold |
| `harness_fea_solver_e2e/stress_sweep_degenerate.rs:417,418` (2) | filters diagnostics by specific message content (`geom_expr` fallback wording), non-exhaustive |
| `purpose_compile_tests.rs:1516,1517,1523` (3) | `guarded_unsupported_member_kind_emits_error` — asserts `!errors.is_empty()` (an error is *already* expected, for the guarded-block's own unsupported-member reason); non-exhaustive |
| `purpose_compile_tests.rs:1567` (1) | doc comment: "the compile test only checks the structural shape (not eval)" — presumed, not directly re-read past the doc comment |
| `harness_traits/trait_assoc_type_conformance_tests.rs:31` (1) | `required_assoc_type_unbound_emits_diagnostic_end_to_end` — filters for a specific code (`TraitAssocTypeNotBound`), non-exhaustive, and the fixture is *already* expected to error |
| `gui/src-tauri/src/tests/engine_tests.rs` (8, across 7 lines) | `EngineSession::load_from_source`/`update_source` — every read test inspects entity-tree/definition/cache structure, never `.diagnostics`; GUI session load is designed to tolerate diagnostics (live editing) — presumed on structural grounds, not a read of `load_from_source`'s own body |

**Not conclusively classified (2):** `trait_assoc_type_conformance_tests.rs`
has 5 total hits; 2 (the override/inherited-default sub-cases, both c1) and
1 (the unbound-Typo sub-case, c0) were read directly, but the file has 2
more occurrences this session did not re-read before writing this section.
Flagged rather than guessed.

**2.2.5 The delta, stated plainly.** 55 measured (17 files) vs. the PRD's 63
(22 files) — short by 8 hits / 5 files. The c2 bucket matches exactly (10/10,
same 4 tests, same file:line spans), which is the strongest available
evidence the *method* is sound; the shortfall is somewhere in c1/c0. Most
likely explanation: this session's tree C/D file lists (reused from pre-2,
itself reusing `corpus_no_bare_scalar.rs`) may not be byte-identical to
whatever file set the PRD's authoring session swept, and/or that session's
"22 files" included a small number of sites this regex's stricter
end-of-match anchor still misses (a residual instance of the same class as
finding 2.2.2's `ts_parser.rs:6488` — one further edge case was found and
fixed there but the search for others was not exhaustive). **Ruling: this
section's 55-site table is the one later leaves should cite** (it is
individually re-verifiable, file:line by file:line, right now, against
HEAD), not the PRD's aggregate 63 — consistent with the citation contract
§10 will state formally.

### 2.3 EXCLUDED-BY-DESIGN — `pub unit`/`unit` bare-factor declarations (addendum C2)

Mandatory bucket per the plan: these are textually identical to category A
(`<keyword> <name> : <Dimension> = <bare number>`) but are unit *conversion
factors*, not value cells — the compiler never diagnoses them (they aren't
`param`/`let`, so neither gate 2 nor gates 3/4 ever see them), and δ₁/γ
"fixing" them would silently break the unit system.

**`crates/reify-compiler/stdlib/units.ri` — 21 `pub unit` declarations**,
verified by `grep -nE "^\s*pub unit\s" stdlib/units.ri`:

```
14  pub unit m    : Length              (base unit, no factor)
15  pub unit cm   : Length = 0.01
16  pub unit in   : Length = 0.0254
17  pub unit ft   : Length = 0.3048
18  pub unit thou : Length = 0.0000254
19  pub unit yd   : Length = 0.9144
23  pub unit rad : Angle                (base unit, no factor)
24  pub unit deg : Angle = 3.141592653589793 / 180
28  pub unit kg  : Mass                 (base unit, no factor)
29  pub unit g   : Mass = 0.001
30  pub unit lb  : Mass = 0.45359237
31  pub unit oz  : Mass = 0.028349523125
35  pub unit s   : Time                 (base unit, no factor)
36  pub unit min : Time = 60
37  pub unit h   : Time = 3600
41  pub unit K    : Temperature         (base unit, no factor)
42  pub unit degC : Temperature = 1 offset 273.15
43  pub unit degF : Temperature = 5 / 9 offset 255.3722222222222
48  pub unit lbf  : Force = 4.4482216152605
53  pub unit psi  : Pressure = 6894.757293168361
54  pub unit ksi  : Pressure = 6894757.293168361
65  pub unit fl_oz : Volume = 0.0000295735295625
66  pub unit gal   : Volume = 0.003785411784
74  pub unit USD : Money                (base unit, no factor)
```

That is 21 lines (6 base units with no `=` at all, 15 with an explicit bare
factor), matching the task premise-verification's "`:14-19`, `:24-25`, and
onward" characterization. **Confirmed present verbatim at HEAD.**

Plus two non-`pub`, example-local `unit` declarations, both confirmed
verbatim:

| File:line | Text |
|---|---|
| `examples/m9_combined.ri:46` | `unit mil : Length = 0.0000254` |
| `examples/integration_full_v01.ri:33` | `unit mil : Length = 0.0000254` |

**None of these 23 sites are in the category A/B/C count above** — they use
the `unit` keyword, not `param`/`let`, so they never reach
`general_leaf_param_family_is_validated`, `check_param_default_conformance`,
or `entity.rs`'s `check_param_default_type`/`check_let_annotation_type` at
all. δ₁ must not "fix" them; γ's flip (already reverted, §1.5) never
touched them either — confirmed no diagnostic was emitted against any
`units.ri` line in the step-1 transcript (§1.2/§1.3).

## §3 Item 7b — §6.3 re-confirmation: gate 1, β/γ's blast radius (step-3)

Unlike §6.2/category-C, §6.3 already gives a full per-site table — this
section's job is genuine re-**confirmation** (direct verbatim re-read of
every cited site at HEAD), not re-derivation, plus the cross-check against
step-1's transcript the plan requires.

### 3.1 The 17 BARE sites — all 17 CONFIRMED present verbatim at HEAD

```bash
sed -n '67p;78p;79p' examples/trajectory/tots_optimal_ptp.ri
sed -n '153p;154p;155p' examples/trajectory/printer_print_envelope.ri
sed -n '1420p' crates/reify-compiler/tests/struct_ctor_field_conformance_tests.rs
sed -n '248p;255p;256p' crates/reify-eval/tests/input_shape_eval_e2e.rs
sed -n '18p;19p;27p;28p;36p;37p' gui/test/fixtures/large_assembly.ri
```

| File:line | Field | Text at HEAD |
|---|---|---|
| `examples/trajectory/tots_optimal_ptp.ri:67` | `JointLimit.max_force` | `let jl = JointLimit(joint: 0.0, max_force: 1000.0)` |
| `examples/trajectory/tots_optimal_ptp.ri:78` | `TOTSShaper.velocity_limit` | `velocity_limit: 300.0,` |
| `examples/trajectory/tots_optimal_ptp.ri:79` | `TOTSShaper.acceleration_limit` | `acceleration_limit: 5000.0,` |
| `examples/trajectory/printer_print_envelope.ri:153` | `JointLimit.max_force` | `actuator_limits: [JointLimit(joint: 0.0, max_force: 1000.0)],` |
| `examples/trajectory/printer_print_envelope.ri:154` | `TOTSShaper.velocity_limit` | `velocity_limit: 300.0,` |
| `examples/trajectory/printer_print_envelope.ri:155` | `TOTSShaper.acceleration_limit` | `acceleration_limit: 5000.0,` |
| `struct_ctor_field_conformance_tests.rs:1420` (×2 args) | pinning probe's own fixture | `let l = Limit(velocity_limit: 300.0, acceleration_limit: 5000.0)` |
| `input_shape_eval_e2e.rs:248` | `JointLimit.max_force` | `let jl = JointLimit(joint: 0.0, max_force: 100.0)` |
| `input_shape_eval_e2e.rs:255` | `TOTSShaper.velocity_limit` | `velocity_limit: 300.0,` |
| `input_shape_eval_e2e.rs:256` | `TOTSShaper.acceleration_limit` | `acceleration_limit: 5000.0,` |
| `gui/test/fixtures/large_assembly.ri:18,27,36` | `Material.density` | `density: 7850.0,` (×2) / `density: 2700.0,` |
| `gui/test/fixtures/large_assembly.ri:19,28,37` | `Material.youngs_modulus` | `youngs_modulus: 200000000000.0` (×2) / `youngs_modulus: 69000000000.0` |

Note one PRD-text imprecision caught by the direct re-read:
`input_shape_eval_e2e.rs:248`'s `max_force` value is **100.0**, not 1000.0
(the `tots_optimal_ptp.ri`/`printer_print_envelope.ri` value) — the PRD's
prose table doesn't actually claim otherwise (it only shows
`tots_optimal_ptp.ri`'s value inline), but a careless reader could
mis-transcribe the fixture from memory; recorded here so no later leaf does.

Per-bucket split reproduced: `examples/**` 6 (tots×3 + printer×3) ·
stdlib `.ri` 0 · Rust fixtures 5 (struct_ctor×2 + input_shape×3) ·
other `.ri` 6 (large_assembly×6) — **CONFIRMED, matches §6.3 exactly.**

**Correct twin, re-verified:** `examples/large_assembly.ri:51-53` reads
`density: 7850kg/m^3,` / `youngs_modulus: 200GPa` — the same assembly,
correctly spelled, confirmed present. β copies this spelling into the GUI
fixture rather than inventing one.

### 3.2 The ~8 excluded false positives — all CONFIRMED present verbatim, file-local-shadowing check reproduced

| File:line | Text at HEAD | Exclusion reason |
|---|---|---|
| `reify-syntax/tests/harness_syntax/auto_binding_sites_lowering_tests.rs:245` | `let source = "structure S { sub b = Bearing(bore: 1.0) }";` | parser-only unit test — `Bearing` is never declared anywhere in this string; the walker cannot run against an undeclared structure |
| `…auto_binding_sites_lowering_tests.rs:360` | `let source = "structure S { let x : Length = Bearing(bore: 1.0) }";` | same — `Bearing` undeclared in this parse-only fixture string |
| `…function_call_named_args_tests.rs:128` | `let expr = first_let_value(r#"structure S { let x = Host(m: Steel(density: 1000.0)) }"#);` | same — `Steel` undeclared in this parse-only fixture string |
| `reify-cli/tests/fixtures/stdlib_sim_ready_material_ok.ri:11` | `param mat : Material = Material(density: 7850.0)` | the fixture declares its **own** dimensionless `Material` structure (shadowing stdlib's dimensioned one) — `density` here is a dimensionless `Real` field, out of the vetted family already |
| `purpose_compile_tests.rs:1814` | `param mat : Material = Material(density: 7850.0)` | same shadowing pattern |
| `purpose_activation.rs:1547` | `param mat : Material = Material(density: 7850.0)` | same shadowing pattern |
| `termination_check_tests.rs:78` | `sub inner = Inner(x: 5)` | file-local `Inner` structure with a dimensionless `x` field, same-name shadow |
| `termination_check_tests.rs:346` | `sub inner = Inner(x: 5)` | same |

**All 8 confirmed present verbatim at HEAD; the file-local-shadowing check
(§6.3's mandatory methodology) is reproduced** — each of the 5
non-parser-only exclusions was verified to declare its *own* structure of
the same name with a dimensionless field, not the stdlib/corpus structure
the name suggests. Without this check a naive sweep over-counts by exactly
this bucket, as §6.3 warns.

### 3.3 Cross-check against the step-1 flipped-predicate transcript

Per the step's mandate ("every BARE site that is compiled by a gate must
appear [in the transcript], and any transcript hit NOT in this table is a
NEW site"):

- **6 of 17** (`tots_optimal_ptp.ri` ×3, `printer_print_envelope.ri` ×3) are
  exercised by `examples_smoke` and appear in its transcript — reconciled
  exactly in §1.2 (7 raw diagnostics = these 6 + the 1 §6.2 category-A site;
  zero unexplained residual).
- **1 of 17** (the ×2-arg `struct_ctor_field_conformance_tests.rs:1420`
  site) is exercised by that suite's own flipped run and appears in its
  transcript, reconciled in §1.3.
- **3 of 17** (`input_shape_eval_e2e.rs:248,255,256`) are exercised by that
  suite under the flip and confirmed **still bare** by direct read (3.1
  above), but do **not** appear as a transcript *failure* because that
  suite only inspects `Severity::Error` and the flip's diagnostics are
  `Severity::Warning` (§1.4's general rule) — their bareness is confirmed
  by source inspection, not by a failing assertion.
- **6 of 17** (`gui/test/fixtures/large_assembly.ri`) are **not exercised by
  any of the four suites step-1 ran** — no suite in this task's flipped run
  loads a `gui/test/fixtures/*.ri` file. Their classification as BARE rests
  on direct source inspection (3.1) plus the walker's documented logic
  (§0 anchor table #2/#4), not on empirical transcript evidence. This is
  the same reach-vs-gate gap step-11 addresses generally: these 6 sites are
  swept by `corpus_no_bare_scalar.rs`'s tree walk but sit behind **no cargo
  test at all** (only `debug_server.rs` and the GUI e2e harness load this
  file) — flagged here as a preview since it bears directly on how β should
  verify its own fix (a cargo test cannot confirm it; the GUI harness or a
  manual `reify check` must).

**No transcript hit fell outside this 17-site table** — zero new BARE sites
found at HEAD.

### 3.4 CROSS-DIMENSION = 0, NON-SCALAR = 0 — corroborated, not exhaustively re-proven

The step-1 transcripts across all four suites surfaced **only** the
already-known BARE/category-A diagnostics (§1.2, §1.3) — no cross-dimension
or non-scalar mismatch fired anywhere in the four suites run under the
flip. This corroborates CROSS-DIMENSION = 0 / NON-SCALAR = 0 **for the
corpus slice those four suites actually exercise** (250 `examples/**` files
+ the two targeted Rust test binaries), consistent with §6.3. It is **not**
a re-proof over the full 595-`.ri`-plus-1707-`.rs` sweep §6.3 originally
ran — reproducing that exhaustively would mean running literally every test
in the workspace under the flip, which this task's scope (four named
suites, per the plan) does not call for.

### 3.5 Aggregate structural counts — inherited, not independently re-derived

**932 structure blocks → 444 distinct structures with ≥1 dimensioned-Scalar
field → 1,157 (structure, field) pairs; 2,770 ctor call sites, of which 474
named args land on a dimensioned-Scalar field; OK = 454.** These figures
require a type-aware analysis (resolving every ctor call's callee to a
structure definition, and that structure's field types, across **both**
`.ri` and inline `.rs` fixture sources) equivalent to re-implementing the
conformance walker itself — outside what a grep-based re-confirmation can
cheaply and reliably reproduce. A coarse sanity check (`grep`-counting bare
`structure`/`structure def` blocks in tracked `.ri` files only, i.e.
excluding the much larger `.rs` inline-fixture population category C had to
sweep separately) found **739** — not comparable to the PRD's 932 since
that figure spans both `.ri` and `.rs`, but not wildly divergent in order of
magnitude either. **Ruling: this section does not re-derive 932/444/1,157/
2,770/474/454 and does not assert them as independently confirmed** — the
decision-relevant artifact for β/γ/δ is the concrete 17-site list (3.1) and
the 8-exclusion list (3.2), both fully re-verified above; a consumer citing
the aggregate counts should cite §6.3 directly, not this ledger.

## §4 Item 1 — Gate 2 hit count (step-4)

*(filled by step-4)*

## §5 Item 2 — Gate 5 (constraint-def Rule 4) sub-counts (step-5)

*(filled by step-5)*

## §6 Item 3 — `Type::ScalarParam` false positives, D4-5 (step-6)

*(filled by step-6)*

## §7 Item 4 — Fn-call-entry reachability, §3.1 (step-7)

*(filled by step-7)*

## §8 Item 5 — Vector3/Point3/Matrix/Tensor/Field quantity-slot residual, §6.4 (step-8)

*(filled by step-8)*

## §9 Item 6 — Load-struct intersection flag for PRD 5, §10.2 (step-9)

*(filled by step-9)*

## §10 Per-site classified ledger + consumer index (step-10)

*(filled by step-10)*

## §11 Methodology closure + no-source-change proof (step-11)

*(filled by step-11)*
