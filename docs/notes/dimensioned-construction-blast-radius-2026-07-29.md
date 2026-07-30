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
- **Date:** 2026-07-29. **Post-review corrections 2026-07-30 (`2b02ef0e5e`, `e242ba6f83`) — two counts GREW; see the §10.9.1 changelog before citing any figure from an earlier revision. No migration disposition changed.**
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
| 5 | Gate 5 / Rule 4 (constraint-def numeric leniency) | `crates/reify-compiler/src/type_compat.rs:1482-1486` | **CONFIRMED** — `let is_numeric = \|t\| matches!(t, Type::Int \| Type::Scalar{..} \| Type::ScalarParam(_)); if is_numeric(param_ty) && is_numeric(arg_ty) { return true }`. Rule 2 at :1474 (`type_carries_type_param \|\| type_carries_trait_object`) already short-circuits before Rule 4. |
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
vectors (stale)"* — i.e. it reads the 51 `(vector, name)` tuples as covering
fewer distinct `DimensionVector` *values*, because a handful of names are
deliberate aliases for the same physical dimension. Three such collisions are
confirmed here by direct inspection: `Stiffness`/`TranslationalStiffness`
share `DimensionVector::STIFFNESS` by explicit const alias at
`dimension.rs:289`; `AbsorptionCoeff`/`Curvature` both compute
`from_exps(&[(0,-1)])` independently and land on the same value;
`Impulse`/`Momentum` literally reuse `DimensionVector::IMPULSE` for two names
at `dimension.rs:593-594`.

> **The quoted "49" is NOT independently re-derived here, and does not
> reconcile — treat it as hearsay, not as a measured figure.** Two things were
> verified in this session: the array holds **51** tuples, and those tuples
> name **50** distinct `DimensionVector` *constant identifiers* (only
> `IMPULSE` is spelled twice). Deducting the three value-collisions documented
> just above from the 51 tuples yields **48** distinct values, not 49 — so the
> other note's figure is off by one against this section's own evidence,
> unless there is a fourth collision (a full distinct-*value* count needs each
> of the 50 constants evaluated, several of which are multi-line and defined
> in terms of others; that was not done, because nothing here depends on it).
> What *is* certain is the shape of the distinction: **"51 names" answers a
> different question than any distinct-vector count** (distinct spellable type
> names vs. distinct physical dimensions), and a reader who conflates the two
> gets a number in the 48-50 band rather than 51. That conflation remains the
> most plausible origin of the plan's "49", alongside the wrapped-tuple regex
> bug reproduced above — but the two candidate origins are **not
> distinguishable from the evidence available**, and this section no longer
> claims one is "more likely".

**RULING (governs every count in §§2-9):** the name set used for the sweep is
**51 names**, because the sweep is a *textual* search over `.ri` type
annotations — what matters is which spellings are legal, not how many
distinct physical dimensions they collapse to. The stale "34" is never used.
The other note's "49 distinct vectors" is retained here only as an
unreconciled explanatory footnote (see the caveat above — it is quoted, not
re-derived, and this section's own evidence points at 48), never as a sweep
input. **No count in §§2-9 depends on any distinct-*vector* figure.**

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
  | grep -vE '^crates/reify-(syntax|ast)/tests/.*\.rs$' \
  | sort -u | wc -l      # -> 2120 at HEAD 08c6c42be9
```

**[post-review] The parse-only exclusion is `.rs`-scoped, and must be.** An
earlier revision published `grep -v '^crates/reify-syntax/tests/\|^crates/reify-ast/tests/'`
— an un-suffixed filter applied to the *whole* union, which additionally drops
two `.ri` fixtures that tree B's 168 counts:
`crates/reify-syntax/tests/fixtures/affine_map_spec_example.ri` and
`crates/reify-syntax/tests/fixtures/sub_placement_spec_example.ri`. That
command emits **2118** at the cited HEAD, not the tabulated 2120 — a published
repro that missed its own stated output. The five tree lists actually used for
the sweep do count them (258 + 168 + 1641 + 41 + 12 = 2120, and their `sort -u`
union is 2120), and this section's own framing ("the two parse-only *dirs*"
exclusion, mirroring `corpus_no_bare_scalar.rs:185-186`) always described a
**Rust-file** exclusion. Measured both ways:

```
$ …| grep -v '^crates/reify-syntax/tests/\|^crates/reify-ast/tests/' | sort -u | wc -l
2118        # published earlier — over-excludes 2 .ri fixtures
$ …| grep -vE '^crates/reify-(syntax|ast)/tests/.*\.rs$' | sort -u | wc -l
2120        # corrected — matches the table and the tree lists
```

**No §2 site is affected**, only the substrate count: the two recovered `.ri`
files hold one dimensioned-looking declaration between them
(`sub_placement_spec_example.ri:18` `param teeth : Count = 24`), and `Count`
is not in §0.3's 65-name set. The table's **2120** stands; the command was
wrong, not the figure.

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
parse-only). My re-measurement: 58 in-scope hits (15 c1 + 10 c2 + 31 c0 + 2
not conclusively classified within this session's budget), drawn from 60 raw
hits across 17 files.** Recorded as a DELTA, not silently reconciled — see
2.2.4.

**[amendment pass] File-count basis.** An earlier revision wrote "58 in-scope
hits across 17 files", pairing two figures measured on **different bases**:
**17** files contain a category-C *hit* (of the 60 raw), but only **15**
contain an *in-scope* one — `let_type_disambiguation_tests.rs` and
`m9_error_cases.rs` hold exactly one hit each and both are 2.2.3 exclusions,
so dropping those 2 hits drops 2 whole files. Cite **58 hits / 15 files** for
the in-scope set and **60 hits / 17 files** for the raw set; never mix them.
2.2.5's "short by 5 files" comparison is 17-vs-22 and stands (both sides count
files containing a hit). Full derivation and the per-row list: **§12**.

**2.2.1 Method.** A line-level scan of every file in tree C
(`crates/**/*.rs`, 1641 files after exclusions) + tree D
(`gui/src-tauri/**/*.rs`, 41 files), adapting `corpus_no_bare_scalar.rs`'s
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

**This is the NORMATIVE stripper** (§10.9 makes this method citable, so a
consumer re-running it must reproduce **58**, not the 55 an earlier revision
of this section reported — see finding (3) in 2.2.2 for why the naive form
under-counts). A `//` starts a comment only when it is *outside* a Rust
string literal:

```python
def strip_trailing_comment(line):
    """`//` starts a comment only OUTSIDE a Rust string literal."""
    i, n = 0, len(line)
    in_string = False
    while i < n:
        c = line[i]
        if in_string:
            if c == '\\':
                i += 2          # skip the escaped char
                continue
            if c == '"':
                in_string = False
            i += 1
            continue
        if c == '"':
            in_string = True
            i += 1
            continue
        if c == '/' and i + 1 < n and line[i + 1] == '/':
            if i == 0 or line[i - 1] != ':':   # original `:` guard, preserved
                return line[:i]
        i += 1
    return line
```

Full runnable substrate, including the buggy original retained side-by-side
for the delta measurement: `/tmp/5756-scratch/cat_c_scan4.py` (invoked as
`cat_c_scan4.py tree-C-filtered.txt tree-D-gui-src-tauri-rs.txt`).

**2.2.2 Three stripper/regex bugs found and fixed while building this —
recorded because a naive version of this sweep silently mis-counts in both
directions.** (1) A
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

(3) **[post-review]** `strip_trailing_comment` truncated the line at the
first `//` whose preceding char is not `:` — **including a `//` that sits
INSIDE a Rust string literal.** So a fixture whose embedded `.ri` snippet
*opens* with a doc/line comment had its entire payload stripped before the
regex ever ran:

```rust
let src = "/// A bracket for mounting.\nstructure Bracket {\n  param w: Length = 1\n}";
//        ^ everything from here on was discarded -> `let src = "`
```

This is a systematic **FALSE-NEGATIVE** class (the opposite direction from
(1)/(2)), and it is *not* point-in-time anchor drift:
`git diff 08c6c42be9 HEAD -- crates/reify-syntax/src/ts_parser.rs` is empty,
so the three missed sites were always there and the sweep never saw them.
Measured with the corrected string-literal-aware stripper (2.2.1), over the
identical tree C + tree D file lists:

```
$ python3 cat_c_scan4.py tree-C-filtered.txt tree-D-gui-src-tauri-rs.txt
OLD_TOTAL_HITS=57  OLD_FILES=17
NEW_TOTAL_HITS=60  NEW_FILES=17
ADDED=3  REMOVED=0
  + crates/reify-syntax/src/ts_parser.rs:6466:let src = "/// A bracket for mounting.\nstructure Bracket {\n  param w: Length = 1\n}";
  + crates/reify-syntax/src/ts_parser.rs:6477:let src = "/// Line one.\n/// Line two.\nstructure S {\n  param x: Length = 1\n}";
  + crates/reify-syntax/src/ts_parser.rs:6499:let src = "// Just a comment\nstructure S {\n  param x: Length = 1\n}";
```

Raw hits go **57 → 60 across the SAME 17 files**; the delta is EXACTLY the
three doc-comment-extraction tests in `ts_parser.rs` (`param w: Length = 1`
once, `param x: Length = 1` twice), and **ZERO hits are removed** — the fix
is **monotone** on this corpus, so it introduces no new false positives.
In-scope total therefore moves **55 → 58** (the 57→60 raw pair less the two
trait-requirement-`let` exclusions of 2.2.3, which are unaffected).

**Spillover bound.** Re-running the corrected stripper over trees A/B/E
(`examples/**/*.ri`, `crates/**/*.ri`, `gui/test/**/*.ri`) yields **1 / 0 / 0
hits both before and after — ZERO delta**, so §2.1's categories A and B are
untouched. §3's BARE table (per-site verbatim re-read) and §8's quantity-slot
census (repo-wide `git grep`) used different methods entirely and are
likewise untouched. **The bug's blast radius is exactly the tree-C/D Rust
sweep, exactly 3 sites.** Full transcript:
`/tmp/5756-scratch/step13-stripper-fix-transcript.txt`.

**2.2.3 Two sites excluded as a different mechanism, not counted in the 58.**
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
| `harness_traits/trait_assoc_type_conformance_tests.rs:110` (override sub-case, `structure_override_assoc_type_populates_template_table`) | `w : Length = 1` | `errors_only(&module)` at `:114` then `errors.is_empty()` at `:116` — exhaustive, "override should compile cleanly" (`:117`) |
| `harness_traits/trait_assoc_type_conformance_tests.rs:166` (inherited-default sub-case, `inherited_default_assoc_type_populates_template_table`) | `w : Length = 1` | same shape — `errors_only` at `:170`, `errors.is_empty()` at `:172`, "inherited default should compile cleanly" (`:173`) |
| `purpose_compile_tests.rs:1453` | `material : Length = 1.0` | `errors.is_empty()` exhaustive |
| `purpose_compile_tests.rs:1454` | `youngs_modulus : Length = 200.0` | same |
| `reify-eval/tests/collection_sub_eval.rs:437` | `grade : Length = 8.8` | manual `.filter(severity==Error)` then `assert!(errors.is_empty())` exhaustive |
| `reify-eval/tests/determinacy_predicates.rs:509` | `a : Length = 10` | via `eval_source(...)` → internally `parse_and_compile`, panics on any Error |
| `reify-eval/tests/purpose_activation.rs:2673` | `material : Length = 100.0` | `parse_and_compile`; doc comment itself notes "panics on any Severity::Error" |
| `reify-eval/tests/purpose_activation.rs:2674` | `youngs_modulus : Length = 200.0` | same |
| `reify-eval/tests/purpose_activation.rs:2774` | `z : Length = 5.0` | same |
| `reify-eval/tests/purpose_activation.rs:2825` | `z : Length = -5.0` | same |

*c0 — no break (31 sites; 11 row-groups).* **[post-review]** Two orthogonal
qualifiers are now carried per row, because §10 renders these rows with the
same authority as the empirically-confirmed ones and a δ₁ reader must be able
to tell them apart:

- **Evidence** — `CONFIRMED` (the enclosing test's assertion was actually
  read) vs `PRESUMED` (disposition inferred on structural grounds; the
  enclosing body was *not* read). Split: **20 CONFIRMED / 11 PRESUMED**. The
  distinction is load-bearing because a c0 disposition turns on whether the
  enclosing test filters to `Severity::Error` (§1.4) — a property of code
  that was not read for the 11.
- **Reachability** — 8 of the 31 are `crates/reify-syntax/src/ts_parser.rs`
  sites that **no conformance gate can ever reach**, so they are
  **EXCLUDED — parse-only**, not BREAKS. See 2.2.4b. The other 23 are
  BREAKS-class-but-unforced ("migrate, lower-urgency").

| File:line(s) | Evidence | Reason |
|---|---|---|
| `let_scope_tests.rs:2025,2471,2540` (3) | CONFIRMED | `.find(...)`/`.any(...)` on specific message content, non-exhaustive |
| `reify-syntax/src/ts_parser.rs:6338,6364,6420,6450` (4) | CONFIRMED (test bodies read) | **parse-only by enclosing-test body** — each site's enclosing `#[cfg(test)]` test calls only a parser entry point and never a compiler one: `:6338` → `lower_port_body_extras_not_flagged` (`fn` at `:6333`) calls `lower_port_body_directly(...)` at `:6337`; `:6364` → `lower_constraint_def_catch_all_emits_for_unexpected_named_children` (`:6358`) via helper `lower_constraint_def_directly` (`:6351`), which calls `ts_parser.parse(...)` at `:6369`; `:6420` → `lower_source_file_catch_all_emits_for_unexpected_named_children` (`:6416`) calls `ts_parser.parse(...)` at `:6425`; `:6450` → `lower_source_file_extras_not_flagged` (`:6446`) calls `parse(source, ModulePath::single("test"))` at `:6451`. No conformance gate runs on them. **EXCLUDED — parse-only**, not BREAKS (2.2.4b) |
| `reify-test-support/src/helpers.rs:1077,1078,1227` (3) | 1227 CONFIRMED · 1077,1078 **PRESUMED** | `1227` confirmed (checks `!result.values.is_empty()`, never inspects diagnostics); `1077/1078` presumed (compile_template's return isn't asserted clean in the slice read) |
| `prelude_context_tests.rs:217` (1) | CONFIRMED | asserts **relative parity** between two compile entry points on the *same* source, not absolute cleanliness — a new diagnostic would land identically on both sides and parity would still hold |
| `harness_fea_solver_e2e/stress_sweep_degenerate.rs:417,418` (2) | CONFIRMED | filters diagnostics by specific message content (`geom_expr` fallback wording), non-exhaustive |
| `purpose_compile_tests.rs:1516,1517,1523` (3) | CONFIRMED | `guarded_unsupported_member_kind_emits_error` — asserts `!errors.is_empty()` (an error is *already* expected, for the guarded-block's own unsupported-member reason); non-exhaustive |
| `purpose_compile_tests.rs:1567` (1) | **PRESUMED** | doc comment: "the compile test only checks the structural shape (not eval)" — presumed, not directly re-read past the doc comment |
| `harness_traits/trait_assoc_type_conformance_tests.rs:31` (1) | CONFIRMED | `required_assoc_type_unbound_emits_diagnostic_end_to_end` — filters for a specific code (`TraitAssocTypeNotBound`), non-exhaustive, and the fixture is *already* expected to error |
| `gui/src-tauri/src/tests/engine_tests.rs:3658,3884,4145,4162,4197,4842,4858` (8, across 7 lines — `4858` matches twice) | **PRESUMED** (all 8) | `EngineSession::load_from_source`/`update_source` — every read test inspects entity-tree/definition/cache structure, never `.diagnostics`; GUI session load is designed to tolerate diagnostics (live editing) — presumed on structural grounds, not a read of `load_from_source`'s own body |
| `reify-eval/tests/eval_param_overrides.rs:1639` (1) | CONFIRMED | `param p: Money = 0`, via non-panicking `compile_source(...)`; the test's assertion ("exactly one Warning mentioning S.p") reads `result_b.diagnostics` from `engine.eval(&module_b)` — the **eval**-phase warning about a param-override dimension mismatch, not `module_b`'s compile-phase diagnostics — so a new compile-time `ParamDefaultTypeMismatch` under δ₁ lands in a different collection than what this assertion inspects. (This is the PRD's own cited "stale rationale" fixture, §6.2; δ₁ retires its doc comment regardless of whether the test itself breaks.) |
| `reify-syntax/src/ts_parser.rs:6466,6477,6488,6499` (4) | CONFIRMED (test bodies read) | same parse-only-by-test-body reasoning as the other 4 `ts_parser.rs` hits — `6488` is the one-line fixture `"structure S {\n  param x: Length = 1\n}"` whose trailing `\n}` (a literal backslash-n, not a real newline) is the edge case finding 2.2.2 (2) fixed the terminator regex for; `6466`/`6477`/`6499` are the three doc-comment-extraction fixtures recovered by finding 2.2.2 **(3)**'s string-literal-aware stripper. All four enclosing tests call `reify-syntax`'s own `parse(src, ModulePath::single("test"))` and nothing else: `doc_comment_on_structure_is_extracted` (`fn` at `:6465`, `parse` at `:6467`), `multi_line_doc_comment_joined` (`:6476`/`:6478`), `no_doc_comment_yields_none` (`:6487`/`:6489`), `regular_comment_not_treated_as_doc` (`:6498`/`:6500`) — each asserts only on the returned module's doc-comment field. No conformance path ever runs on them. **EXCLUDED — parse-only** (2.2.4b) |

**Evidence roll-up (11 PRESUMED):** `helpers.rs:1077,1078` (2) ·
`purpose_compile_tests.rs:1567` (1) · `engine_tests.rs` (8). Everything else
in c0 is CONFIRMED — 31 − 11 = **20**. Of the 23 c0 sites that are *not*
parse-only, the split is **12 CONFIRMED / 11 PRESUMED**; all 8 parse-only
sites are CONFIRMED **by a read of each of the 8 enclosing test bodies**
(anchors in the two `ts_parser.rs` rows above).

> **Retraction [amendment pass].** An earlier revision marked these 8 rows
> `CONFIRMED (deps read)` and justified them by a read of
> `crates/reify-syntax/Cargo.toml`, calling that "a stronger and cheaper form
> of evidence, since it settles the whole file at once". **Both halves of that
> claim were wrong** and are withdrawn: see 2.2.4b. A dependency read settles
> nothing about which functions any individual test in a file calls, so it is
> strictly *weaker* than a body read, not stronger — and the premise it rested
> on is false besides. Only the per-body evidence above supports these 8 rows.

**2.2.4b [post-review] The 8 `ts_parser.rs` sites are EXCLUDED — parse-only,
not BREAKS.** An earlier revision folded all 31 c0 sites into a single
"BREAKS — migrate, lower-urgency" disposition in §10.2/§10.3, which put the
8 `ts_parser.rs` sites two rows away from `mv-2-priv-param.ri:4` — a row
whose disposition is `EXCLUDED — parse-only` — under **substantially the same
reasoning and the opposite verdict**.

**RETRACTED JUSTIFICATION (read this before citing an earlier revision).** The
revision that first published this ruling justified it from the *dependency
graph*: it quoted `crates/reify-syntax/Cargo.toml` and concluded that
"`reify-compiler` appears on **neither** list, so these 8 in-crate unit tests
cannot reach gate 2 … by any path". The quoted block was verbatim-correct but
**the inference from it is FALSE, and is withdrawn.** `[dev-dependencies]`
lists `reify-test-support.workspace = true`, and
`crates/reify-test-support/Cargo.toml:14` carries `reify-compiler.workspace =
true` as a **normal** dependency. Dev-dependencies are linked into a crate's
in-crate `#[cfg(test)]` unit tests, so `reify-compiler` **IS** transitively
available to `ts_parser.rs`'s test module — and that module already reaches
into `reify-test-support` today, demonstrated in-file at `ts_parser.rs:4525`
(also `:4746`, `:4941`, `:5058`, `:5576`), each calling
`reify_test_support::bracket_source()`. A crate-level dependency read
therefore cannot establish parse-only-ness for any site in this file.

**JUSTIFICATION AS IT NOW STANDS — per-test-body, and independently
re-verified at HEAD.** All 8 sites sit inside `#[cfg(test)]` tests whose
bodies call **only** a parser entry point — `parse(...)`,
`ts_parser.parse(...)`, or `lower_port_body_directly(...)` — and never a
compiler entry point, so no conformance gate runs on them **regardless of what
is linked into the test binary**. Per-site `fn`/call anchors are tabulated in
the two `ts_parser.rs` rows above (`fn` at `:6333`, `:6358`, `:6416`, `:6446`,
`:6465`, `:6476`, `:6487`, `:6498`). Corroborating whole-file check: the only
two textual occurrences of `reify_compiler` in `ts_parser.rs` are a doc-link
in a comment at `:52` and a prose mention at `:3963` — there is no call to it
anywhere in the file.

This is a *reachability-by-callee* argument, which is the same **kind** of
argument that excludes `mv-2-priv-param.ri:4` (a `tree-sitter-reify` grammar
fixture whose only consumers are parse-only grammar tests) — but note it is
**not** the same *strength*: this one is established site-by-site from 8 test
bodies, whereas the earlier revision wrongly claimed a single crate-level read
settled the whole file at once. **Ruling (unchanged): the two must be
classified identically.** All 8 move to `EXCLUDED — parse-only`; §4.1's gate-2
candidate count, §10.2's and §10.3's sub-totals, and §10.8's δ₁ row are
restated accordingly. This *shrinks* the normative BREAKS counts — it never
adds work.

**The retraction changed no disposition and no count.** Every one of the 8
sites was re-verified by a body read and every one is still
`EXCLUDED — parse-only`; §4.1's 47, §10.2's 46+9, §10.3's 39, and §10.8's 23
all stand exactly as published. A consumer holding the amended figures has
**nothing to re-plan** — only the *reason* recorded beside them changed. See
§10.9.1's changelog row for this pass.

**No disposition moves into or out of c1/c2, and no site leaves the 58.** The
8 remain category-C hits and remain c0; only their *disposition label* is
corrected from BREAKS-lower-urgency to EXCLUDED-parse-only. **β/γ/δ₁/δ₂ must
not re-plan off this correction** — δ₁'s optional-cleanup list simply gets 8
items shorter, and every one of the 8 was provably incapable of changing a
diagnostic in the first place.

**DISPOSITION IMPACT OF FINDING 2.2.2 (3) IS NIL.** All three newly-recovered
sites are the same **structurally-parse-only c0** class as the
already-accounted `ts_parser.rs:6488` — folded into that row rather than
added as new row-groups, which is why c0 moves **28 → 31 sites while the c0
row-group count stays 11**. No consumer's migration work changes: nothing
moves into or out of c1/c2, no BREAKS row is added, no DELIBERATE-INVERT row
is added. **β/γ/δ₁ must not re-plan off this correction** — only the ledger's
counts move. (Under 2.2.4b those three are now EXCLUDED-parse-only rather
than BREAKS-lower-urgency, which makes their impact less than nil: they
subtract from the migration list rather than adding to it.)

**Not conclusively classified (2):** `trait_assoc_type_conformance_tests.rs`
has 5 total hits — `:31`, `:76`, `:110`, `:166`, `:224`. Three were read
directly: `:110` (override sub-case, c1), `:166` (inherited-default sub-case,
c1), and `:31` (unbound sub-case, c0). The remaining **`:76`
(`required_assoc_type_satisfied_by_default_no_diagnostic`) and `:224`
(`structure_binding_to_nonexistent_type_emits_diagnostic`)** are the 2 this
session did not re-read before writing this section. Flagged rather than
guessed; **file:line given so a consumer resolving the pair knows exactly
which two occurrences are still open and which three are already accounted
for.**

**2.2.5b `param` vs `let` split (needed by step-4's gate-2 attribution).**
Of the **58** in-scope hits, **54 are `param` declarations and 4 are `let`**
(all three of finding 2.2.2 (3)'s newly-recovered sites are `param`; the 4
`let`s are exactly the `let_annotation_type_mismatch_tests.rs` c2 sites — every
other hit, across all of c1/c0/c2, is a `param`). This matters because gate
2 (`conformance/mod.rs`'s `check_param_default_conformance`) is
**`param`-only** per §2.1's gate table — the `let` twin lives at gate 4
(`entity.rs:563-569`) only, a different mechanism α's flip never touches.
Verified by re-scanning with a capture group on the `param|let` keyword
rather than inferring it from table membership.

**2.2.5 The delta, stated plainly.** **58** measured (17 files *containing a
category-C hit* — the basis the PRD's own file count uses; 15 contain an
in-scope hit, §12.2) vs. the PRD's 63 (22 files) — short by **5** hits / 5 files. The c2 bucket matches exactly (10/10,
same 4 tests, same file:line spans), which is the strongest available
evidence the *method* is sound; the shortfall is somewhere in c1/c0. Most
likely explanation: this session's tree C/D file lists (reused from pre-2,
itself reusing `corpus_no_bare_scalar.rs`) may not be byte-identical to
whatever file set the PRD's authoring session swept, and/or that session's
"22 files" included a small number of sites this regex's stricter
end-of-match anchor still misses (a residual instance of the same class as
finding 2.2.2's `ts_parser.rs:6488` — one further edge case was found and
fixed there but the search for others was not exhaustive) — finding 2.2.2
**(3)** later closed 3 of the original 8-hit shortfall from exactly that
class, which is corroborating evidence for this explanation. **Ruling: this
section's 58-site table is the one later leaves should cite** (it is
individually re-verifiable, file:line by file:line, right now, against
HEAD), not the PRD's aggregate 63 — consistent with the citation contract
§10 will state formally.

### 2.3 EXCLUDED-BY-DESIGN — `pub unit`/`unit` bare-factor declarations (addendum C2)

Mandatory bucket per the plan: these are textually identical to category A
(`<keyword> <name> : <Dimension> = <bare number>`) but are unit *conversion
factors*, not value cells — the compiler never diagnoses them (they aren't
`param`/`let`, so neither gate 2 nor gates 3/4 ever see them), and δ₁/γ
"fixing" them would silently break the unit system.

**`crates/reify-compiler/stdlib/units.ri` — 24 `pub unit` declarations**,
re-runnable at any HEAD by:

```bash
grep -cE "^\s*pub unit\s" crates/reify-compiler/stdlib/units.ri   # -> 24
grep -nE  "^\s*pub unit\s" crates/reify-compiler/stdlib/units.ri  # -> the 24 rows below
```

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

That is **24** lines, decomposing as **6 base units with no `=` at all**
(`m`:14, `rad`:23, `kg`:28, `s`:35, `K`:41, `USD`:74) **+ 15 with a single
bare-literal factor + 3 with an expression/offset factor** (`deg`:24 =
`3.141592653589793 / 180`, `degC`:42 = `1 offset 273.15`, `degF`:43 =
`5 / 9 offset 255.3722222222222`) — 6 + 15 + 3 = 24. **Confirmed present
verbatim at HEAD.**

> **Correction (post-review).** An earlier revision of this section said "21
> lines (6 base + 15 bare factor)", silently dropping the three
> expression/offset forms — even though the verbatim listing directly above
> it always showed all 24 rows. It was the prose total that was wrong,
> contradicted by the section's own quoted evidence; the listing was never
> short. All 24 are equally EXCLUDED-BY-DESIGN: the expression/offset forms
> are no more a `param`/`let` than the bare-factor ones, so none of them
> reaches gate 2 or gates 3/4 either. The premise-verification's
> "`:14-19`, `:24-25`, and onward" characterization is also imprecise and is
> superseded by the anchor list above: line 25 is blank, and `rad` is line
> **23**, not part of a `:24-25` span.

Plus two non-`pub`, example-local `unit` declarations, both confirmed
verbatim:

| File:line | Text |
|---|---|
| `examples/m9_combined.ri:46` | `unit mil : Length = 0.0000254` |
| `examples/integration_full_v01.ri:33` | `unit mil : Length = 0.0000254` |

**None of these 26 sites** (24 stdlib `pub unit` + 2 example-local `unit`)
**are in the category A/B/C count above** — they use
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

Gate 2 = `conformance/mod.rs:517-532`'s `check_param_default_conformance`
(`param` defaults only — the `let` twin is gate 4, `entity.rs:563-569`,
untouched by this predicate). Never separately measured before this task.

### 4.1 Candidate count

Every §2 category-A/C site whose keyword is `param` (not `let`) is a gate-2
candidate: **54 (Category C, Rust fixtures) + 2 (Category A, `.ri`) = 56**,
minus **9 parse-only** = **47 sites where the compiler, if it compiles
that file under the flip, newly emits a gate-2 `Warning`/`ArgTypeMismatch`.**
Split: `examples/**` 1 (`bearing_auto_seal.ri`) · stdlib `.ri` 0 ·
Rust fixture strings 46 · other `.ri` 0 (§2's Category A/C tables list every
site; not reproduced again here).

**The 9 parse-only exclusions [post-review].** `mv-2-priv-param.ri:4` (a
tree-sitter grammar fixture, never reaches `reify-compiler` at all) **plus
the 8 `crates/reify-syntax/src/ts_parser.rs` sites** (`:6338,6364,6420,6450,
6466,6477,6488,6499`), which 2.2.4b re-verified are unreachable by the same
*kind* of argument — each of the 8 enclosing `#[cfg(test)]` tests calls only a
parser entry point (`parse` / `ts_parser.parse` / `lower_port_body_directly`)
and never a compiler one, so gate 2 cannot run on them. (2.2.4b **retracts**
an earlier dependency-graph justification for these 8: `reify-compiler` *is*
transitively linked into `reify-syntax`'s unit-test cfg via the
`reify-test-support` dev-dependency. The disposition and every count below are
unaffected.) An earlier revision of this section subtracted only the 1, which
over-stated the candidate set by 8; the correction **shrinks** the count
(55 → 47) and adds no work anywhere.

Of these 47, **step-1's actual flipped run empirically confirms exactly 1**
— `bearing_auto_seal.ri:46`, diagnostic `argument 'durometer' has type
'Real' but param 'durometer' requires type 'Scalar[m]'`, reconciled in
§1.2 as the union's non-ctor-call member. The other 46 sit in Rust test
binaries this session did not compile under the flip (step-1 ran 4 named
suites, not the whole workspace), so their gate-2 firing is **mechanical,
not empirically re-confirmed this session** — the walker doesn't
distinguish "is anyone testing this," it fires whenever
`general_leaf_param_family_is_validated` returns `true` for that field's
type and the arg's inferred type differs, which is true of a bare `Real`
literal against every one of these 46 declared-dimensioned params by
construction.

### 4.2 BREAKS vs DELIBERATE-NEGATIVE-TEST-TO-INVERT — gate 2 specifically

**Structural finding: gate 2's blast radius on the *existing test suite* is
far narrower than gates 3/4's, because gate 2 emits `Severity::Warning`
only, and every helper this task found gating a §2 "c1" (gates-3/4
would-break) classification filters to `Severity::Error` before asserting**
— `parse_and_compile`/`eval_source` (`collect_errors`, Error-only, read in
§2.2.4's investigation), `errors_only`, and every inline
`.filter(|d| d.severity == Severity::Error)` this task read. A new
gate-2 Warning is invisible to all of them. Consequence: **of the 15 §2
"c1" sites (gates 3/4 would-break), zero additionally break because of gate
2** — their failure mode under δ₁ is orthogonal to γ's flip. This is the
test-suite-level analogue of §3.1/§11's "γ is severity-safe pre-δ" claim,
extended from "cannot break compilation" to "cannot break an
`errors_only`-shaped assertion."

The **only** test this session found (empirically, via step-1's real run)
that breaks because of gate 2 specifically:

| Test | File | Assertion | Result under flip |
|---|---|---|---|
| `no_example_emits_ctor_field_conformance_diagnostics` | `examples_smoke.rs` | corpus-wide scan for **any** ctor-conformance diagnostic (Warning included) | RED, 7 diagnostics (§1.2) |
| `excluded_family_dimensioned_scalar_given_dimensionless_real_is_silent` | `struct_ctor_field_conformance_tests.rs:1414-1461` | "must emit **ZERO** ctor-conformance diagnostics" | RED (§1.3) — **DELIBERATE NEGATIVE TEST TO INVERT**, the canonical example already named in §1.3 |

No other test read in §2 (including the 4 `param_default_type_mismatch_tests.rs`
c2 pins) is gate-2-sensitive: those 4 assert on
`DiagnosticCode::ParamDefaultTypeMismatch` specifically, a **different
code** from gate 2's `ArgTypeMismatch`, produced by a **different**
mechanism (`entity.rs`, gates 3/4) — a new gate-2 Warning wouldn't match
their filter. They remain gate-3/4 pins (§2.2.4), not gate-2 pins.

**BREAKS = 1** (`bearing_auto_seal.ri`, already migrated in spirit by β
per the PRD's note that its fix is the *annotation*, gate 3, not a gate-2
migration — β does not need to do anything for gate 2 at this site beyond
what it already does for gate 3). **DELIBERATE-NEGATIVE-TEST-TO-INVERT = 1**
(the struct_ctor pinning test). Every other gate-2 candidate site is
silent-break-free at the *test* level (no currently-passing assertion
notices), even though the compiler now emits a diagnostic there.

### 4.3 `auto`/`auto(free)`/`undef` silence — addendum C6, measured empirically

Probe file (`/tmp/5756-scratch/probes/auto_undef_probe.ri`, scratch-only,
never committed):

```
module auto_undef_probe

structure Q { param s1 : String = auto
              param free_len : Length = auto(free) }
structure R { param own_default : Length = undef }
structure P { param r : Length = undef
              param s : String = undef
              param l : Length = undef }
structure Main { sub q = Q()
                 sub r = R()
                 sub p = P(r: undef, s: undef, l: undef) }
```

Run against `target/debug/reify check` — **UNFLIPPED** (baseline, before
any edit this step):

```
warning: W_UNDERDETERMINED: auto parameter 'Q.s1' in scope 'Q' is not touched
by any constraint (touching constraints: none); its value is underdetermined (free)
All constraints satisfied.
```

Then the local flip was applied to `conformance/mod.rs`, `reify-cli`
rebuilt (`RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0 cargo build -p
reify-cli --bin reify`, 8m45s cold), and the **identical** probe re-run
**FLIPPED**:

```
warning: W_UNDERDETERMINED: auto parameter 'Q.s1' in scope 'Q' is not touched
by any constraint (touching constraints: none); its value is underdetermined (free)
All constraints satisfied.
```

**Byte-identical output.** `Q.s1 = auto`, `Q.free_len = auto(free)`,
`R.own_default = undef` (an unoverridden `undef` **default**, exercising
gate 2 directly), and `P(r: undef, s: undef, l: undef)` (an explicit
`undef` **ctor arg**, exercising gate 1) all stay completely silent under
BOTH predicate states. **Answer: NO — `auto`/`auto(free)`/`undef` are
judged at neither walker entry, confirmed empirically, not assumed.** The
96 `auto`/`auto(free)`/`undef` default sites §6.2 counted are safe from
γ's flip. The flip was then reverted
(`git checkout -- crates/reify-compiler/src/conformance/mod.rs`) and
`git diff --exit-code -- crates/ examples/ gui/ stdlib` re-confirmed clean
before this section was written; `target/debug/reify` was left built from
the flipped source at probe time (target/ is gitignored, not part of the
no-source-change invariant) and should be rebuilt before any further use
that depends on baseline behavior.

**Anti-cascade guard, recorded as a distinct silence source
(`conformance/mod.rs:519-521`):** the `Type::Error` early-return in gate
2's `_ =>` arm exists so a param whose type **already failed to resolve**
(an unrelated upstream error) doesn't ALSO get a cascading ctor-conformance
diagnostic layered on top. This guard is orthogonal to
`general_leaf_param_family_is_validated` — it fires on `Type::Error`
(a resolution failure), never on a validly-resolved dimensioned `Scalar`.
A later reader auditing "why is site X silent" must check which of the two
independent silences applies; conflating them would misattribute a
resolution-failure silence to the family predicate.

## §5 Item 2 — Gate 5 (constraint-def Rule 4) sub-counts (step-5)

Gate 5 = Rule 4 of `constraint_arg_type_conforms` (`crates/reify-compiler/src/type_compat.rs:1482-1488`), the numeric-leniency
carve-out inside the **constraint-instantiation** (`constraint <ConstraintDefName>(arg: expr, …)`) argument check, called from
`expand_constraint_inst` (`entity.rs:5835`, plus the forall per-element expansion path `forall_elaborate.rs:532`). Never
separately measured before this task. **Original measurement, not re-confirmation** — reported per the plan as two
independent numbers, never merged.

### 5.0 Mechanism boundary — this is a SEPARATE predicate from step-1's flip; step-1's transcript does not cover it

`constraint_arg_type_conforms` (`type_compat.rs`) and `general_leaf_param_family_is_validated`
(`conformance/mod.rs`, the predicate α flips in step-1) are **different functions in different files**, gating
**different member kinds** — struct-ctor/param-default conformance (gates 1-4, walked by `walk_param_against_arg`) vs.
constraint-**instantiation** argument conformance (gate 5, walked by nothing — a flat per-arg check, no recursion). Flipping
the step-1 predicate touches zero lines of Rule 4 and vice versa; step-1's transcript (§1) contains **no gate-5 evidence
at all** and cannot be reused here (unlike item 1/gate 2, which did reuse it). This section required its own independent
empirical sweep, run and reverted separately from step-1's flip (never overlapping in the working tree at the same time).

### 5.1 Method — instrumented empirical sweep, not grep alone

Unlike §§2-4's grep-over-name-set method, gate 5's corpus cannot be graded by text search alone: `constraint_arg_type_conforms`
is reached only for **named constraint-definition instantiations** (`constraint Foo(x: …)`), which are syntactically
indistinguishable by grep from same-shaped **constraint-expression members whose body happens to be a struct-ctor call**
(`constraint Widget(label: 42) == Widget(label: 42)` — `Widget` is a `structure def`, not a `constraint def`; found and
excluded live, §5.1.3) and from several **compiler-intrinsic constraint kinds** (`RepresentationWithin`, `AllParamsDetermined`,
`AllGeometryDetermined`, `Coincident`, …) that are never registered as a `constraint def` anywhere in the tree (confirmed by
repo-wide `git grep -n "constraint def RepresentationWithin"` → zero hits) and are handled by dedicated runtime interception
(`crates/reify-eval/src/engine_constraints.rs:301-398`, "RepresentationWithin interception") — bypassing `expand_constraint_inst`'s
per-param loop entirely. A grep-only sweep cannot tell these apart from a real instantiation without independently
re-deriving type resolution. So this section uses the same class of method step-1 used for the main predicate: a temporary,
local, reverted instrumentation of the rule itself, whose output is authoritative because it fires only when the real
compiler pipeline reaches Rule 4 — never a false positive.

**5.1.1 Instrumentation (temporary, never committed).**

```diff
--- a/crates/reify-compiler/src/type_compat.rs
+++ b/crates/reify-compiler/src/type_compat.rs
@@ -1485,6 +1485,7 @@
     let is_numeric = |t: &Type| matches!(t, Type::Int | Type::Scalar { .. } | Type::ScalarParam(_));
     if is_numeric(param_ty) && is_numeric(arg_ty) {
+        eprintln!("RULE4_HIT param={:?} arg={:?}", param_ty, arg_ty); // task-5756 probe, reverted before commit
         return true;
     }
```

**5.1.2 Runs.** Every non-parse-only crate whose test corpus contains the string `constraint def ` or a named
`constraint <Name>(` call was identified by repo-wide `git grep` (49 `.ri`+`.rs` files spanning `reify-compiler`,
`reify-eval`, `reify-doc-build`; zero hits in `gui/**`, `docs/prds/**`, `tests/prd-gate/**`, or `tree-sitter-reify/**` —
confirmed by the same un-scoped `git grep -n "constraint def "` used in §0's premise check, which already covers the
whole repo, not just the `corpus_no_bare_scalar.rs` trees). Each run redirected full output to a file and used
`--nocapture` (`eprintln!` is otherwise swallowed for passing tests) with `--test-threads=1` (a `--test-threads=8` first
pass produced 2 lines with garbled/interleaved text — libtest's documented `--nocapture` caveat, concurrent threads
writing directly to the shared fd with no serialization across tests; the serial re-run reproduced the same 7 raw hits
byte-clean with an unambiguous preceding `test <name> ...` line for every one):

| Run | Command | Result |
|---|---|---|
| `reify-compiler`, full suite | `cargo test -p reify-compiler --tests -- --nocapture --test-threads=1` | 4624 passed, 0 failed, 2m49s |
| `reify-eval`, constraint-relevant targets (17 test binaries named for every file this session found referencing `constraint def`/`constraint <Name>(` in this crate) | `cargo test -p reify-eval --test constraint_def_eval --test integration_corner_cases --test integration_full_v01 --test m11_full_integration --test m8_m11_regression_checkpoint --test m9_combined --test m9_constraint_def --test m9_integration --test optimized_registry_tests --test test_runner --test unified_dag_geometry_executors --test tolerance_member_access_e2e --test representation_within_assertion --test determinacy_intrinsics --test determinacy_integration_gate --test dfm_fits_build_volume_e2e --test harness_fea_solver_e2e -- --nocapture --test-threads=1` | 825-line log, 0 failed, 3m41s |
| `reify-eval`, `tests/common/differential.rs`-including targets (shared fixture module, not itself a target) + `reify-doc-build` | `cargo test -p reify-eval --test harness_selective_demand --test unified_dag_boundary_cases --test unified_dag_differential_corpus --test unified_dag_edit_path -p reify-doc-build --test build_doc_model_tests -- --nocapture --test-threads=1` | 14 passed, 0 failed, 14s |
| `reify-eval`, `broad_corpus_sweep` (evaluates the same `examples/**` corpus `examples_smoke` already compiles) | started full `--tests`, killed after 21/24 shards (>30 min — geometry/FEA-kernel-backed, no new information: 0 hits through shard 21, and Rule 4 is a compile-time-only check already exercised on the identical files by `examples_smoke` above) | 0 hits observed before stop; not exhaustively completed — see caveat below |

**Caveat on the killed run:** the `reify-eval` full-suite run was stopped after shard 21/24 of `broad_corpus_sweep`
(no failures, no RULE4_HIT observed) because it re-evaluates the identical `examples/**` files `examples_smoke`
(`reify-compiler`, above) already **compiles** — and Rule 4 is a pure compile-time type check, insensitive to what the
eval engine does afterward. `examples_smoke` produced zero gate-5 hits over the full 250-file exercised set (§5.1.2's
first row), so the killed run's partial 0/21 is corroborating, not load-bearing. **This one corner (the un-exercised
3/24 shards plus whichever `reify-eval` fixtures outside the 21 targeted files above were never run) is the one gap this
section's sweep does not claim to close exhaustively** — recorded per the plan's own "no silent caps" discipline rather
than folded into the clean counts below.

**5.1.3 Grep false positives found and excluded while scoping the runs** (recorded so a later reader does not
re-discover them): (a) `struct_ctor_field_conformance_tests.rs:715,751` — `constraint Widget(label: 42) == Widget(label: 42)`
is a **constraint-expression member** (a boolean `==` expression whose two operands are struct-ctor calls), not a
constraint-definition instantiation; `Widget` is `structure def Widget { param label : String }`, not a `constraint def` —
this is gate-1 territory (already in §3's ctor-conformance table), textually identical to a gate-5 call only by
coincidence of the shared `constraint <Capitalized>(` shape. (b) `crates/reify-eval/src/graph.rs`'s `constraint Some(id_0)`
is Rust pattern-matching syntax (`Option::Some`), not Reify source at all — a bare substring match artifact. (c)
`crates/reify-syntax/tests/harness_syntax/constraint_inst_tests.rs:57` (`constraint Bounded(lo: 1, hi: t, x: t)`) is a
parse-only syntax fixture (`reify-syntax` crate, `tests/harness_syntax/` — the same parse-only exclusion established in
§0.4/§3.2) — it never reaches `reify-compiler`. (d) `crates/reify-compiler/tests/constraint_inst_tests.rs:303`
(`constraint OneParam(a: x, b: 5)`, in `unknown_argument_name`) has `b` matching **no declared param** of `OneParam`
(which only declares `param a`) — `entity.rs:5828`'s `let Some(param) = def.params.iter().find(...) else { continue; }`
skips unmatched arg names before `constraint_arg_type_conforms` is ever called, so this is excluded by a **different**
mechanism (unknown-arg skip), not by Rule 4.

### 5.2 Severity structural finding, load-bearing for how these counts are read

Gate 5's rejection (Rule 5, the `false` fallthrough) emits `Diagnostic::error(...)` (`entity.rs:5838`) — confirmed
`Severity::Error` at the constructor (`crates/reify-core/src/diagnostics.rs:3888-3896`), **not** `Severity::Warning`
like gates 1-4's `CTOR_FIELD_CONFORMANCE_SEVERITY`. Consequence, the mirror image of §4.2's gates-3/4 finding: because
gate 5 is Error-severity, **any** `errors_only`/`error_diags`-filtered assertion or panicking `compile_source`/`eval_source`
helper is *already* sensitive to a new gate-5 diagnostic — there is no Warning-shaped blind spot for δ₂ to hide behind.
Both corpus sites found below (§5.4) sit inside exactly such error-sensitive tests; **this sweep found zero "silently
tolerant" (c0-shaped) gate-5 corpus sites** — a structural contrast with gates 3/4's §2.2, where the c0 bucket (31 sites)
was the majority.

### 5.3 Sub-count (a): CROSS-DIMENSION scalar-for-scalar — **0 corpus/integration sites, 2 primitive-level pins**

A `Scalar{Q}` arg reaching a `Scalar{R}` constraint-def param with Q ≠ R (including Q = dimensionless), tolerated only by
Rule 4 — confirmed by direct read of `type_compatible`/`implicitly_converts_to` (`type_compat.rs:52-281`): the only
Scalar-relevant branches are `from == to` (identity — same dimension) and Int→dimensionless-scalar widening (arg `Int`,
param dimensionless); neither covers a differently-dimensioned or dimensionless **Scalar** arg against a dimensioned param,
so that case falls through past Rule 3 to Rule 4 with no other path to acceptance.

**Zero occurrences across the entire instrumented sweep (§5.1.2) come from `.ri`/Rust-fixture corpus content.** The
**only** two firings of this shape are pure Rust unit tests of the primitive function itself — no `.ri`-shaped source,
no `compile_source` call, direct `Type` value construction:

| File:line | Test | Types | Disposition |
|---|---|---|---|
| `crates/reify-compiler/src/type_compat.rs:5041` | `constraint_arg_type_conforms_mass_for_length_is_true` | param=`Scalar{Length}`, arg=`Scalar{Mass}` | **PRIMITIVE PIN — TO INVERT** (asserts `conforms(...)` is `true`; δ₂ removing this half flips the expectation) |
| `crates/reify-compiler/src/type_compat.rs:5050` | `constraint_arg_type_conforms_dimensionless_for_length_is_true` | param=`Scalar{Length}`, arg=`Scalar{dimensionless}` | **PRIMITIVE PIN — TO INVERT** |

Bucket split: `examples/**` 0 · stdlib `.ri` 0 · Rust fixture strings 0 · other `.ri` 0 · **primitive unit-test pins on
the rule itself 2** (a class the standard four-bucket taxonomy doesn't have a slot for, called out explicitly rather than
force-fit — these two tests exist to specify Rule 4's own contract, not to exercise a compiled `.ri` corpus site).
BREAKS = 0 (no corpus site depends on this half incidentally); DELIBERATE-NEGATIVE-TEST/PIN-TO-INVERT = 2.

### 5.4 Sub-count (b): INT-FOR-LENGTH — **2 corpus/integration sites (4 firings), 1 primitive-level pin**

A `Type::Int` arg reaching a dimensioned `Scalar` constraint-def param — confirmed distinct from Rule 3's Int-for-**dimensionless**
widening (which requires the param itself to be dimensionless; a dimensioned param never qualifies), so this is
Rule-4-only exactly as the plan states.

| File:line | Test | Site (source) | Firings | Bucket | Disposition |
|---|---|---|---|---|---|
| `crates/reify-compiler/src/type_compat.rs:5031` | `constraint_arg_type_conforms_int_for_length_is_true` | direct `Type` values, no source | 1 | primitive pin | **PRIMITIVE PIN — TO INVERT** |
| `crates/reify-compiler/tests/constraint_def_compile_tests.rs:1454` | `int_literal_for_length_param_no_constraint_arg_type_mismatch` (fn at :1447) | `constraint def MinWall { param w: Length … }` / `constraint MinWall(w: 3)` | 1 | Rust fixture string | **DELIBERATE-NEGATIVE-TEST-TO-INVERT** — the test's own name and body (`assert_eq!(code_count, 0, "… numeric leniency — dimensional strictness is task 4490's job …")`) exist specifically to pin this tolerance |
| `crates/reify-compiler/tests/forall_statement_lower_tests.rs:1646` | `forall_constraint_inst_body_emits_per_element_inst_predicates` (fn at :1639, def at :1641-1644) | `constraint def MinThreshold { param value : Length … }` / `forall v in [1, 2, 3]: constraint MinThreshold(value: v)` | **3** (one per forall element — `v` = `1`, `2`, `3`, each a separate `Type::Int` arg) | Rust fixture string | **BREAKS** — the test's own `assert!(errors_only(&module).is_empty(), …)` (its first assertion, before the label-mechanics checks it actually exists to pin) would newly fail under δ₂; its *purpose* is forall per-element constraint-label mechanics, not gate-5 tolerance, so the fix is migrating the fixture's literals to unit-bearing ones (e.g. `[1mm, 2mm, 3mm]`), not inverting an assertion the way the row above does |

Bucket split: `examples/**` 0 · stdlib `.ri` 0 · Rust fixture strings 2 sites / 4 firings · other `.ri` 0 ·
primitive unit-test pin 1. BREAKS = 1 site (3 firings, `forall_statement_lower_tests.rs`); DELIBERATE-NEGATIVE-TEST/PIN-TO-INVERT
= 2 (the `constraint_def_compile_tests.rs` site + the primitive pin).

**5.4.1 Total reconciliation.** 2 (cross-dimension primitive pins) + 1 (int-for-length primitive pin) + 1 (int-for-length
`constraint_def_compile_tests.rs` firing) + 3 (int-for-length `forall_statement_lower_tests.rs` firings) = **7 raw
`RULE4_HIT` firings, matching the instrumented sweep's total exactly** — zero unaccounted residual.

### 5.5 Structural note (i) — `Type::ScalarParam` admission is a δ₂ prerequisite, not resolved here

`is_numeric` (`type_compat.rs:1485`) admits `Type::ScalarParam(_)` alongside `Int`/`Scalar{..}`. Verified by direct read
that neither `type_compatible` nor `implicitly_converts_to` (`type_compat.rs:52-281`, the full text of Rule 3's
transitive callees) branches on `Type::ScalarParam` anywhere — so a `Scalar<Q>` (dimension-generic) arg reaching a
**concretely-dimensioned** constraint-def param structurally falls through Rules 1-3 and is accepted **only** by Rule 4,
exactly the same shape of gap item 3/step-6 investigates for gate 1. **Zero corpus sites exercise this today** — the
three known dimension-generic sites (`stdlib/fields.ri:156/160/164/193/197`, `examples/generics/dim_param.ri`,
`tree-sitter-reify/test/fixtures/guf-2-bounded.ri`) were checked directly for `constraint` instantiation content:
`dim_param.ri`'s only `constraint` members are bare boolean predicates (`constraint len > 29.99mm`, four total, none a
named instantiation); `fields.ri`/`guf-2-bounded.ri` have none. So this is a **latent** property of Rule 4, not a
measured break — flagged, per the plan, as a prerequisite δ₂ must carry its own fence for (if δ₂ removes Rule 4 wholesale
without a ScalarParam-aware replacement, a *future* dimension-generic constraint arg would newly and incorrectly reject),
not something this task resolves.

### 5.6 Structural note (ii) — Rule 2 precedes Rule 4; Rule-2-excluded sites must never be counted against Rule 4

Confirmed by direct re-read of the sequential early-return structure (`type_compat.rs:1464-1491`): Rule 2
(`type_carries_type_param(param_ty) || type_carries_trait_object(param_ty)`, `:1474`) is a single `if { return true }`
textually and structurally **before** Rule 4's `if` block (`:1482-1488`) in the same straight-line function — a
generic/trait-typed **param** (e.g. `constraint def Aligned<T> { param t: T, param w: Length … }`, confirmed compiling
cleanly with zero "unknown type" errors at `constraint_def_compile_tests.rs:990-1010`, `generic_constraint_def_with_type_param_type_compiles_cleanly`)
never reaches Rule 4 regardless of its arg's type. This governs attribution, not a count change here: no site in §5.3/§5.4
was excluded by Rule 2 (none of the found sites have a generic/trait-typed **param** — `Aligned<T>`'s own param `t: T` is
never instantiated with an arg in that test, so it contributes zero firings either way), but a later reader (δ₂) auditing
"why is site X silent under Rule 4" must check Rule 2 first — misattributing a Rule-2 generic-param skip to Rule 4's
numeric leniency would overstate δ₂'s blast radius.

### 5.7 Revert proof

```
$ git diff --stat -- crates/
 crates/reify-compiler/src/type_compat.rs | 1 +
 1 file changed, 1 insertion(+)
$ git checkout -- crates/reify-compiler/src/type_compat.rs
$ git diff --exit-code -- crates/ examples/ gui/ stdlib
$ echo $?
0
```

No diagnostic/behaviour change survives this section; `type_compat.rs:1482-1488` reads identically to HEAD both before
and after this measurement.

## §6 Item 3 — `Type::ScalarParam` false positives, D4-5 (step-6)

**Provenance note for §§6-11:** main was rebased under this task between step-5 and step-6
(`2bf2e858` → `842bc79b`, 23 files — `docs/debug-mcp-contract.md`, `design-invariants.md`,
the sibling `units-gating-gap-research` note, `dimension-checked-readers.md`,
`reify-stdlib-reference.md`, `gui/test/visual/**`, `scripts/setup-*`, `tests/infra/**`). **Zero
overlap** with any file this ledger's counts depend on (`conformance/mod.rs`, `type_compat.rs`,
`dimension.rs`, `examples/**`, `stdlib/**`, `gui/test/fixtures/**`, `tree-sitter-reify/**`) —
confirmed by direct diff of the changed-file list against §0's reuse-target trees, so §§0-5's
figures need no re-verification. §§6-11 are measured fresh at the post-rebase HEAD
**`e6479597d7`** (2026-07-30) — every anchor this section cites was re-read directly from disk
in this session, not carried over from before the rebase.

D4-5's own text (PRD §5): *"`Type::ScalarParam` args are accepted through
`is_numeric_placeholder_leaf`, NOT by widening `arg_type_is_unverifiable`."* This is a design
decision **γ must implement** — a description of required future behavior, not a claim about
what HEAD does today. This section measures both halves the plan's addendum C5 requires kept
separate, plus the mechanism connecting them (original measurement — not previously written
down by anyone).

### 6.0 Mechanism, traced through the actual code

Two structurally independent silences are at stake; only one requires the flip to observe.

- `arg_type_is_unverifiable` (`conformance/mod.rs:1134-1139`, the skip-list `reject_if_incompatible`
  at `:1194-1201` consults before calling `type_compatible`) **deliberately excludes**
  `Type::ScalarParam` — its own doc comment (`:1122-1133`) says adding it "would silence
  genuine family-level mismatches such as `String ← Scalar<Q>` at every arm at once."
- `type_compatible` (`type_compat.rs:220-281`) → `implicitly_converts_to` (`:52-179`) has **no
  `Type::ScalarParam` arm anywhere** (confirmed by direct read of the full function body).
  `is_scalar_like_leaf` (`:37-50`), the allowlist gating Rules 2a/2b/2c, also excludes
  `Type::ScalarParam` (only `Bool|Int|String|Scalar{..}|Enum|TypeParam|StructureRef|TraitObject|Geometry`).
  So for `from = ScalarParam("Q")`, `to = Scalar{any dimension}`: identity fails (different
  variant), no Rule matches, both directions fall to the final `_ => false`. **`type_compatible(Scalar{anything}, ScalarParam(_))`
  is `false`, unconditionally — nothing downstream distinguishes dimensioned from
  dimensionless once the general-leaf arm is entered.**

Consequence: whether a `ScalarParam` arg misfires against a concrete `Scalar` param depends
**entirely** on whether `general_leaf_param_family_is_validated(param_type)` is `true` for
that param. **Today** that holds for `Bool|Int|String` and dimensionless `Scalar` only → the
dimensionless/`String` half already misfires (§6.3, live, no flip needed). **Post-flip** it
holds for every `Scalar` → the dimensioned half would newly misfire too (§6.2, demonstrated
with the same local, reverted flip step-1/step-5 used). D4-5's `is_numeric_placeholder_leaf`
fence (`:1141-1155`) is real but wired into exactly the `Point`/`Matrix`/`Tensor` shape-based
arms (task 5465) today — **never** into the general concrete-leaf arm gates 1/2/6 share. γ
must add that wiring; it does not exist at HEAD.

### 6.1 The dimensioned half — census of every dimension-generic definition site: 0 forwarding sites

Every dimension-generic `fn`/structure/alias in tracked source, found by
`git grep -nE 'fn\s+\w+\s*<[^>]*:\s*Dimension'` and
`git grep -nE '(structure def|pub type|type)\s+\w+\s*<[^>]*:\s*Dimension'` (`.ri` only —
the same queries against `*.rs` return zero hits; dimension-bounded generics are never spelled
inside a Rust fixture string in this corpus):

| Site | Kind | Forwards `Scalar<Q>` into a CONCRETE dimensioned ctor field/param default? |
|---|---|---|
| `stdlib/fields.ri:156` `clamp_field<D,Q:Dimension>(f: Field<D,Scalar<Q>>, lo, hi: Scalar<Q>)` | fn | **NO** — body `fn_field(\|p\| clamp(sample(f,p), lo, hi))`; all generic eval-builtins, no ctor call |
| `stdlib/fields.ri:160` `remap_field<D,Q:Dimension>(...)` | fn | **NO** — body `fn_field(\|p\| remap(sample(f,p), from_lo, from_hi, to_lo, to_hi))`, same shape |
| `stdlib/fields.ri:164` `threshold<D,Q:Dimension>(f: Field<D,Scalar<Q>>, value: Scalar<Q>) -> Field<D,Bool>` | fn | **NO** — body `fn_field(\|p\| sample(f,p) > value)`, a comparison |
| `stdlib/fields.ri:193` `pointwise_max<D,Q:Dimension>(...)` | fn | **NO** — body `fn_field(\|p\| max(sample(f,p), sample(g,p)))` |
| `stdlib/fields.ri:197` `pointwise_min<D,Q:Dimension>(...)` | fn | **NO** — same shape as `pointwise_max` |
| `examples/generics/dim_param.ri:15` `scale_q<Q:Dimension>(x: Scalar<Q>, k: Real) -> Scalar<Q> { x * k }` | fn | **NO** — returns `Scalar<Q>` from arithmetic, no ctor call; both call sites (`scale_q(10mm,3.0)`, `scale_q(5MPa,2.0)`) bind `Q` to a concrete dimension AT the call, so any value reaching a ctor downstream is already an ordinary `Scalar{Length\|Pressure}`, never a live `ScalarParam` |
| `tree-sitter-reify/test/fixtures/guf-2-bounded.ri:1` `clamp_field<D,Q:Dimension>(...)` | fn | **NO** — byte-identical body to `fields.ri:156`; also parse-only (§0.4/§3.2 exclusion, never reaches `reify-compiler`) |
| `tests/fixtures/parametric_alias_def_site_ok.ri:19` `structure def Box2<T: Dimension> { param x : Real }` | structure | **N/A — `T` is never used.** The only field is hardcoded `Real`; `T`'s bound exists solely to test the def-site alias-guard (task 4796). No `Scalar<T>` value is ever constructed. |
| `tests/fixtures/parametric_alias_def_site_reject.ri:17` `structure def Holder<T: Dimension> { param x : Real }` | structure | **N/A**, same reason as `Box2`; additionally this whole fixture is a deliberately-invalid negative test (filename `_reject`) that never compiles |
| `stdlib/trajectory.ri:103` `Vec3<Q:Dimension>`, `stdlib/units.ri:106` `Rate<Q:Dimension>`, plus `Vel`/`Wrap`/`LeakName`/`BadBound` in the two `parametric_alias_def_site_*.ri` fixtures | type aliases | **N/A — not this item.** Aliases have no body to forward a value from. `Vec3` is item-5/§8 territory (Vector family); `Rate` is already recorded in §0.2 as a *bare-literal-default* blind spot (a future `param v : Rate<Length> = 5` site — item 7a/gates 2-4 territory, not arg-forwarding) |

**Count: 0 of 7 dimension-generic `fn` sites forward a `Scalar<Q>` into a concrete dimensioned
ctor field or param default — matching the independent decompose-time count exactly.** Every
`fields.ri` combinator only ever hands its generic scalar to another generic builtin (`clamp`,
`remap`, `sample`, comparison, `max`/`min`) or returns it directly; neither dimension-bounded
structure (`Box2`, `Holder`) actually types a field with its own bound type param.

### 6.2 The dimensioned half — empirically confirmed reachable-if-exercised

Because §6.1 finds zero corpus exposure, "the fence is one line away from live code" needs its
own proof independent of any corpus site. Probe
(`/tmp/5756-scratch/probes/scalarparam_dimensioned_probe.ri`, scratch-only, never committed) —
a shape §6.1 confirms no existing site takes:

```
module scalarparam_dimensioned_probe

structure ConcreteSink {
    param p : Length
}

fn fwd_dim<Q: Dimension>(x: Scalar<Q>) { ConcreteSink(p: x) }
```

```
$ target/debug/reify check scalarparam_dimensioned_probe.ri     # BASELINE (unflipped, HEAD e6479597d7)
All constraints satisfied.
$ echo $?
0
```

Local flip applied (identical diff to step-1's repro block, `Type::Scalar { .. } => true` at
`conformance/mod.rs:1691-1697`), `RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0 cargo build -p
reify-cli --bin reify` (1m, warm sccache — the pre-flip object was already cached from
step-1/step-5's own cycles), re-run:

```
$ target/debug/reify check scalarparam_dimensioned_probe.ri     # FLIPPED
warning: argument 'p' has type 'Scalar<Q>' but param 'p' requires type 'Scalar[m]'
All constraints satisfied.
$ echo $?
0
```

**Confirmed live: the flip alone turns this into a FALSE-POSITIVE Warning.** `x`'s true nature
(a not-yet-resolved dimension-generic value, correct at every valid instantiation of `Q`) is
indistinguishable, at this arm, from a genuinely wrong-dimension `Real`. This is exactly the
shape D4-5 requires γ to fence off before promoting the predicate for real. Flip reverted
immediately after (`git checkout -- crates/reify-compiler/src/conformance/mod.rs`), confirmed
clean (§6.4), `reify` rebuilt from the clean tree and re-confirmed silent on this same probe
before §6.3 was run (baseline restored — the exact trap step-4 flagged, deliberately avoided
here).

**Severity note:** both runs are gate 1 (`ConcreteSink(p: x)` is a structure-constructor call,
`check_trait_arg_conformance`, `CTOR_FIELD_CONFORMANCE_SEVERITY` = Warning) — this item never
touches gate 6 (fn-call entry, Error), which is §7's subject.

### 6.3 The dimensionless/`String` half — live misfire TODAY, no flip required, verbatim per the plan's mandate

Probe (`/tmp/5756-scratch/probes/scalarparam_dimensionless_misfire.ri`, scratch-only, never
committed) — the plan's own literal example, reproduced exactly:

```
module scalarparam_dimensionless_misfire

structure Sink {
    param r : Real
    param s : String
}

fn fwd<Q: Dimension>(x: Scalar<Q>) { Sink(r: x, s: x) }
```

(`Sink` is declared locally in this probe module — stdlib separately declares an unrelated
marker `trait Sink {}` at `io.ri:61`; the two never collide, the same file-local-shadowing
precedent §3.2 already documents for `Material`/`Inner`.)

```
$ target/debug/reify check scalarparam_dimensionless_misfire.ri     # BASELINE — HEAD e6479597d7, unflipped
warning: argument 'r' has type 'Scalar<Q>' but param 'r' requires type 'Real'
warning: argument 's' has type 'Scalar<Q>' but param 's' requires type 'String'
All constraints satisfied.
$ echo $?
0
```

**Confirmed verbatim, exactly as the plan predicted** (`'Scalar<Q>' … requires type 'Real'` /
`'Scalar<Q>' … requires type 'String'`). Re-run under the flip (§6.2) for completeness —
byte-identical output (`r`/`s` are already-vetted-family params; the flip only affects
DIMENSIONED `Scalar` params, and `Real` is dimensionless, so this pair is flip-insensitive —
consistent with I5's contract holding both pre- and post-γ).

**This confirms I5's own contract ("the fence is not a blanket") is already exercised, today,
by this exact live misfire** — no PRD work is needed to observe it. Owner: whoever implements
D4-5's fence (γ) — it must silence `Scalar{Q1} ← ScalarParam` (dimensioned-vs-dimensioned)
WITHOUT silencing `Real ← ScalarParam` or `String ← ScalarParam` (genuine family mismatches),
which is exactly why D4-5 mandates `is_numeric_placeholder_leaf` (a scalar-vs-non-scalar test)
over widening `arg_type_is_unverifiable` (which would blanket-silence by argument type alone).

### 6.4 Disposition, corpus impact, and revert proof

**BREAKS = 0. DELIBERATE-NEGATIVE-TEST-TO-INVERT = 0.** No corpus site (`examples/**`, stdlib
`.ri`, Rust fixture strings, other `.ri`) exercises the dimensioned-generic-forwarding shape
today (§6.1); both probes (§6.2, §6.3) are scratch-only, never committed. This reconciles
exactly with step-1's transcript by absence: its 7 raw diagnostics (§1.2/§1.3) are already
fully accounted for as known BARE/category-A sites, zero of them `ScalarParam`-shaped —
corroborating §6.1's census over the `examples/**`+2-suites slice step-1 actually exercised.

**This is a γ-scoped implementation prerequisite, not a corpus migration item** — there is no
site for β to migrate. Recorded for γ: before promoting the predicate for real, wire
`is_numeric_placeholder_leaf` into the general concrete-leaf arm's skip path (or an equivalent
fence), or gates 1/2/6 will newly false-positive on any FUTURE dimension-generic fn that
forwards its scalar into a concrete field — a regression `examples_smoke`'s corpus gate would
not catch today (zero sites exist to trip it) but would catch immediately once one exists, per
§6.2's live demonstration.

```
$ git diff --exit-code -- crates/ examples/ gui/ stdlib
$ echo $?
0
```

Both probe-driven build cycles in this section are fully accounted for; `target/debug/reify`
was rebuilt clean immediately after §6.2's revert and re-confirmed silent before §6.3 ran, so
§6.3's "no flip required" claim rests on a verified-baseline binary, not a stale flipped one.

## §7 Item 4 — Fn-call-entry reachability, §3.1 (step-7)

Reported, per the plan's own instruction, as a **measurement at HEAD**, not a structural
guarantee — the PRD's zero-reachability argument is half wrong (addendum C1, re-confirmed in
§0's anchor table rows 6-8), so this section re-derives the consequence from the corrected
guard structure rather than repeating the PRD's reasoning.

### 7.0 Guard structure — cross-reference, not re-derivation

Already fully re-verified in §0's anchor table (rows 6-8): guard (i)
`type_carries_trait_object(param_ty)` (`entities_phase.rs:1493`) precedes the only production
`check_fn_arg_conformance` call (`:1503`); guard (ii)'s candidate backstop —
`resolve_function_overload`'s filter (`type_compat.rs:1194-1201`) — leads with the **same**
predicate (`:1196`, first disjunct), so the two guards are mutually exclusive, not conjunctive,
and a param satisfying guard (i) is mechanically reachable. `OverloadResolution::Resolved` is
gated at `entities_phase.rs:1486-1488` (the PRD's `:1508-1511` is stale drift). This section
measures the consequence: does any corpus/fixture site place a bare dimensioned `Scalar` leaf
where guard (i) lets a call through?

### 7.1 Mechanical reachability, demonstrated live (the plan's own worked example)

Probe (`/tmp/5756-scratch/probes/fncall_entry_reachability.ri`, scratch-only, never committed),
reproducing the plan's example with a real stdlib trait (`MaterialSpec`, `materials_mechanical.ri:67`)
so the value side of the map genuinely conforms and the diagnostic below is the ONLY one:

```
module fncall_entry_reachability

structure def Steel : MaterialSpec {
    param density : Density = 7850kg/m^3
    param name : String = "steel"
}

fn takes(m : Map<String, MaterialSpec>) -> Int { 1 }

structure def Top {
    let result = takes(map{ 1 => Steel() })
}
```

```
$ target/debug/reify check fncall_entry_reachability.ri     # HEAD e6479597d7, unflipped — no flip needed for this demo
error: argument 'm' has type 'Int' but param 'm' requires type 'String'
$ echo $?
1
```

**Confirmed verbatim, exactly as the plan predicted, EXIT 1.** `Map<String, MaterialSpec>`
satisfies guard (i) (`type_carries_trait_object` recurses into `Map`'s value position,
`:423` `Type::Map(key, val) => type_carries_trait_object(key) \|\| type_carries_trait_object(val)`,
hits `TraitObject("MaterialSpec")`), so `check_fn_arg_conformance` runs; the walker recurses
lockstep into the Map's **key** position (`conformance/mod.rs:679-684`) and finds `1` (`Int`)
against the declared `String` — an already-vetted-family mismatch, `general_leaf_param_family_is_validated(String)`
being unconditionally `true` today, needing no flip. `ctx.arg_name` stays `'m'` for the whole
recursive walk (never reassigned per wrapper position), which is why the message names the
whole param rather than a per-entry position.

**Severity confirms the contract table (§0/§7.2 of the PRD):** `check_fn_arg_conformance`
(`conformance/mod.rs:359-382`) builds its own `WalkCtx` with `severity: Severity::Error`
hard-coded (`:380`, doc comment: *"Fn-call trait-conformance is OUT OF SCOPE for the 5302 ctor
knob and must stay a hard error"*) and calls the **same** `walk_param_against_arg` gates 1/2
use (`:381`) — a leaf firing here is Error/exit-1, unlike gates 1-4's Warning/exit-0.

### 7.2 Repo-wide census: 0 fn params both trait-carrying and recursing to a bare dimensioned-`Scalar` leaf

**Method.** Two independent sweeps, because the target shape (a `fn` param that is BOTH
`type_carries_trait_object` AND recurses lockstep to a bare `Type::Scalar` leaf — e.g.
`Map<Scalar<Q>, SomeTrait>`) needs the wrapper syntax AND a trait name in the same param.

**(a) `.ri` files — every `fn` decl with a `Map`/`List`/`Set`/`Option` wrapper param:**

```bash
git grep -nE '^\s*(pub\s+)?fn\s+\w+' -- '*.ri' | grep -E 'Map\s*<|List\s*<|Set\s*<|Option\s*<'
```

19 raw hits, all inspected directly: every one wraps a `TypeParam` (`T`/`K`/`V`, e.g.
`option_recovery.ri`'s `unwrap_or<T>(o: Option<T>, ...)`, `get_or<K,V>(m: Map<K,V>, ...)`) or a
concrete non-trait leaf (`List<JointValue>` — `JointValue` is the `= Real` alias from §0.2, not
a trait; `List<Length>`, `List<Pose3>`, `List<Profile>`… — none is a declared trait). **Zero**
wrap a trait name.

**(b) `.ri` files — every `fn` decl with a BARE (unwrapped) trait-named param**, cross-checked
against the full repo-wide declared-trait-name list (`git grep -hoE '^\s*(pub\s+)?trait\s+[A-Za-z_]\w*'
-- '*.ri'`, 100 names; `T` excluded from the alternation below — it is itself a declared
one-letter trait name somewhere in a fixture and collides with every generic `<T>`, a false-positive
mechanism confirmed by inspection, not a real trait-typed param):

| Param shape | Sites | Wrapper? |
|---|---|---|
| `feature: Geometry` (`stdlib/tolerancing.ri:334`); `g: Geometry` (×3, `reify-eval/tests/fixtures/{fea_bc_box,morph_box,volume_mesh_box}.ri`) | 4 | bare |
| `p: Profile` (`stdlib/trajectory.ri:382,386,391,873`); `profile: Profile` (`:873`, second param); `p: Profile` (`trajectory_fns.ri:50`) | 6 | bare |
| `shaper: Shaper` (`stdlib/trajectory.ri:873`) | 1 | bare |

All bare, single-position — the walker recurses to the `TraitObject` leaf directly with no
Scalar co-located in the same type, matching the plan's predicted "bare `Trait` or `List<Trait>`
→ single-position → lockstep bottoms out on the TraitObject" bucket exactly (the `List<Trait>`
half of that prediction turned up zero live instances; only the bare half is populated).

**(c) `.rs` fixture strings — repo-wide, wrapper + trait name in the same param:**

```bash
TRAITS=$(git grep -hoE '^\s*(pub\s+)?trait\s+[A-Za-z_]\w*' -- '*.ri' | grep -oE '\w+$' | sort -u | grep -vx T | paste -sd'|')
git grep -nE 'fn\s+\w+\s*(<[^>]*>)?\s*\([^)]*(Map|List|Set|Option)\s*<[^)]*\b('"$TRAITS"')\b[^)]*\)' -- '*.rs'
```

Two distinct shapes found:

- **`fn couple_opt(joint : Option<DrivingJoint>) -> Real`** — `crates/reify-compiler/tests/harness_traits/fn_arg_trait_conformance_tests.rs:137,417,460`
  (one fixture pattern, reused across 3 test functions). `DrivingJoint` is a real trait
  (`kinematic.ri:88`). Confirmed by direct read: `Option`'s single inner position IS the trait
  leaf — no co-located Scalar. This is the SAME reachability mechanism §7.1 demonstrates (a
  test in this exact file, `option_wrapped_trait_param_bad_arg_emits_conformance_error`,
  already pins gate-6-through-`Option` firing) but contributes 0 to this item because there is
  no dimensioned leaf in the type.
- **`fn takes_faces(g: List<Geometry>) -> Int`** — `param_binding_selector_coercion_tests.rs:27` —
  **false lead, excluded on inspection.** This file (task 4118 γ, selector coercion) is about
  the BUILT-IN `Type::Geometry` primitive (the same type `type_compatible`'s
  `(Type::List(inner), Type::Selector(_)) if matches!(inner.as_ref(), Type::Geometry)` rule at
  `type_compat.rs:255-259` names), not the stdlib `trait Geometry {}` — confirmed by the file's
  own doc comment ("`Selector(k)` argument fed to a `List<Geometry>` function parameter") and
  its `faces_by_normal(...)` call site. `type_carries_trait_object` has no arm for
  `Type::Geometry` (only `TraitObject`/`Option`/`List`/`Set`/`Map`/`Applied`/`Projection`
  recurse; everything else, including `Geometry`, falls to `_ => false`) — so this param fails
  guard (i) regardless, corroborating rather than contradicting the census.

Also checked: `fn wrap<T: Rigid>(items: List<T>) -> List<T>` (`fn_generic_trait_bound_tests.rs:312`)
is a **trait-BOUNDED generic type param**, structurally different from a trait-object leaf —
its declared type is `List(TypeParam("T"))`, and `type_carries_trait_object` does not match
`TypeParam` at all, so this also fails guard (i) regardless of `T`'s bound.

**(d) The `Map<Trait, leaf>` decls in `harness_traits/trait_typed_param_tests.rs`** — the plan's
own named check. All 5 occurrences (`:771,802,958,1493,1547`) read directly:
`structure def Host { param ms/m : Map<String, MaterialSpec> }` (×3),
`Map<String, M>` (×1), `Map<MaterialSpec, String>` (×1) — **every one is a STRUCTURE ctor
param** (`structure def Host { param ... }`), gate 1, `CTOR_FIELD_CONFORMANCE_SEVERITY` =
Warning — **confirmed excluded from the fn-call-entry (gate 6) census**, exactly as the plan
states.

**Measured count: 0.** No `fn` parameter in the tracked corpus (`.ri` files or `.rs` fixture
strings) both carries a trait object and recurses lockstep to a bare dimensioned `Type::Scalar`
leaf.

### 7.3 Consequence statement — the escalation clause is NOT triggered, and why that matters

Measured **0** at HEAD `e6479597d7`. Per the plan's mandate: *"If NON-ZERO, those sites are
Error-severity pre-γ … and MUST be migrated in β BEFORE γ lands … additionally file it to the
escalation channel."* Since the count is 0, **no β-ordering change and no escalation are
filed.** This is a re-confirmation of the PRD's corroborating claim ("0 bare-at-dimensioned
user-fn call sites in the entire `.ri` corpus"), now independently re-derived from a repo-wide
structural sweep (§7.2) rather than inferred from the silence of four suites (§1.4's
`reify-eval` finding already established that suite-level GREEN is not evidence of anything —
this section does not repeat that mistake for gate 6). **Stated per the plan's own framing
(design decision, §0): this is a measurement at this HEAD, not a structural guarantee** — §7.1
proves the mechanism is live and reachable in general (the `couple_opt`/`takes` demos both
fire); §7.2's 0 is a fact about today's corpus contents, not about the guard's structure, and
must be re-run if this ledger is ever cited against a later HEAD.

### 7.4 Revert proof

No `crates/`, `examples/`, `gui/`, or `stdlib` file was modified for this step — every probe in
§7 is scratch-only (`/tmp/5756-scratch/probes/`) and every transcript above ran against the
already-baseline (unflipped) binary left in place at the end of §6.4.

```
$ git diff --exit-code -- crates/ examples/ gui/ stdlib
$ echo $?
0
```

## §8 Item 5 — Vector3/Point3/Matrix/Tensor/Field quantity-slot residual, §6.4 (step-8)

**Scope boundary, stated precisely before any count (conflating the two halves below would
overstate what γ closes).** The walker recurses lockstep through `Option`/`List`/`Set`/`Map`
(`conformance/mod.rs:669-701`), so a dimensioned `Scalar` **wrapped** in one of those IS judged
by the general-leaf arm and IS covered by γ — e.g. `stdlib/kinematic.ri:130-132`
(`Option<TranslationalStiffness>` etc., already the PRD's own worked example). By contrast,
`Vector3`/`Point3`/`Matrix`/`Tensor`/`Field` (task 5465's four shape-based arms —
Vector/Point share one arm keyed on their common `n`+`quantity` shape, the PRD's "four
promoted families" count) accept a scalar-family arg via `is_numeric_placeholder_leaf`
(`conformance/mod.rs:1141-1155`) in their OWN dedicated arms and therefore stay
**dimension-blind in the quantity slot** — γ's predicate promotion never reaches them; **this
PRD does not change that.** This section counts the residual so 5627 candidate-2 /
`reify-core/src/ty.rs`'s quantity-slot follow-up can be sized — the number is **deliberately
out of scope for this PRD (§12)** and is not a migration list.

### 8.1 Worked contrast, re-verified (the PRD's own anchor pair)

| Site | Type | Covered by γ? |
|---|---|---|
| `stdlib/kinematic.ri:130-132` `Prismatic.spring_rate/damping/neutral` | `Option<TranslationalStiffness>` / `Option<TranslationalDamping>` / `Option<Length>` | **YES** — `Option` wrapper, walker recurses to the `Scalar` leaf |
| `stdlib/dynamics.ri:79` `MassProperties.inertia` | `Matrix<3, 3, MomentOfInertia>` | **NO** — `Matrix`'s own shape-based arm, `is_numeric_placeholder_leaf`-gated, dimension-blind |

Both re-confirmed verbatim at HEAD `e6479597d7` (also re-verified independently at decompose
time, per this task's premise-verification analysis).

### 8.2 Census method and results

`git grep -noE ':\s*(Vector3|Point3|Matrix|Tensor|Field|Vec3)\s*<[A-Za-z0-9_, ]*'` (the `Vec3`
alias included, per §0.2's `Vec3<Q> = Vector3<Q>` finding), repo-wide, then per-hit
classification: (i) strip comment-only lines (doc prose mentioning a type is not a
declaration); (ii) exclude a **dimensionless or generic** quantity slot (`Dimensionless`,
`Real`, or a bare type-param letter `T`/`Q`/`D`/`A`/`B`/`C`/… — dimension-blindness has no
observable consequence there, since there is no dimension to get wrong); count only a
**concrete named dimension** in the quantity slot.

**stdlib `.ri` — 34 sites, 9 files** (all `param`, except `joints.ri`'s 5, which are typed
params of a `joint <name>(...)` template definition, a distinct DSL construct from
`structure def`/`fn` but the same type-position question):

| File | Sites | Fields |
|---|---|---|
| `constitutive.ri:54-57` | 4 | `origin`, `x_axis`, `y_axis`, `z_axis` : `Point3<Length>`/`Vector3<Length>`×3 |
| `dynamics.ri:78-79` | 2 | `MassProperties.com : Point3<Length>`, `.inertia : Matrix<3,3,MomentOfInertia>` (§8.1) |
| `fdm.ri:112` | 1 | `build_direction : Vec3<Length>` |
| `joints.ri:59,128,141` | 5 | `revolute`'s `p`, `spherical`'s `c`+`d`, `ball`'s `c`+`d` : `Point3<Length>` |
| `kinematic.ri:129,147,157,164,165` | 5 | `Prismatic`/`Revolute`/`Cylindrical`'s `axis`, `Planar`'s `axis_x`+`axis_y` : `Vec3<Length>` |
| `ports.ri:54-57` | 4 | `Frame3`'s `origin`/`x_axis`/`y_axis`/`z_axis` : `Vec3<Length>` |
| `ports_mechanical.ri:139,152` | 2 | two port `axis : Vector3<Length>` fields |
| `solver_buckling.ri:176` | 1 | `mode_shape : Field<Point3<Length>, Vector3<Length>>` |
| `solver_elastic.ri:500,501,519,531,541,542,631,632,633,769` | 10 | `displacement`/`stress`/`divergence`/`gradient`/`curl`/`frame`/`top`/`mid`/`bottom`/`material`, all `Field<Point3<Length>, …>` (domain always dimensioned; codomain varies) |

**`examples/**` — 8 sites, 4 files:** `joint_dof_self_check.ri:26` (1, `joint revolute`'s `p`) ·
`parametric_vec3_cross_module.ri:20` (1, `traction : Vec3<Pressure>`) ·
`stdlib/ports_breadth.ri:78-81` (4, `Vector3<Length>`) ·
`type_hygiene/type_hygiene_surface.ri:26,66` (2, `moi : Tensor<2,3,MomentOfInertia>`).
(`dimensionless_unification.ri:50-51`'s `Vector3<Real>`/`Vector3<Dimensionless>` pair excluded
— dimensionless, no residual.)

**Other `.ri` — 4 sites, 3 files** (`tests/prd-gate/fixtures/`, outside `corpus_no_bare_scalar.rs`'s
reach, §11(A)): `compiler_type_hygiene_integration_gate.ri:34,40` (2) ·
`compiler_type_hygiene_mul_scale_guard_defeat.ri:14` (1) ·
`compiler_type_hygiene_mul_vec_silent_int.ri:12` (1) — all `Vector3<Length>`.
(The two `tree-sitter-reify/test/fixtures/guf-*.ri` hits, `Field<D,Scalar<Q>>`/`Field<A,B>`,
are generic, not concrete — excluded, and parse-only regardless per §0.4/§3.2.)

**Rust fixture strings — 72 code-position sites across 21 files** (repo-wide `git grep` over
`*.rs`, restricted to `tests/` paths — non-test `.rs` **implementation** files, e.g.
`type_compat.rs`'s own `Tensor<1,N,Q>` Rust-generic signatures or `units.rs`'s internal
helpers, are a different question entirely — Rust-level type machinery, not `.ri` source — and
are excluded; comment/doc-prose mentions, 64 of the 136 raw distinct-line hits, are excluded
the same way §2.2's sweep excludes them). Top files by hit count:
`harness_langcore/parametric_vector_point_resolution_tests.rs` (10),
`struct_ctor_field_conformance_tests.rs` (9), `comparison_operand_guard_tests.rs` (7),
`datum_constructor_tests.rs` (5), `harness_langcore/type_hygiene_integration_gate.rs` (5),
`joint_dof_self_check_tests.rs` (4); 15 further files with 1-3 each (full file list and
per-line hits: `/tmp/5756-scratch/step8-rs-concrete-testsonly.txt`, scratch, uncommitted). **Per
the plan's own framing ("the number exists solely to size the follow-up") this bucket is
counted, not individually re-classified BREAKS-vs-pinned site-by-site** — unlike items 1/2/3,
because this residual is explicitly out of scope for γ/δ and no β migration follows from it.
Flagged per the ledger's no-silent-caps discipline: this is the one count in the ledger not
carried to single-site granularity, and the reason is the PRD's own scope ruling, not sweep
laziness.

### 8.3 Total and disposition

**118 concrete-dimensioned quantity-slot sites** (34 stdlib + 8 examples + 4 other-`.ri` + 72
Rust-fixture) will **remain dimension-blind after γ lands** — none is touched by this PRD, none
is a β migration item, and none is a δ leaf. This number is strictly **larger** than any other
item's corpus figure in this ledger (§3's 17 BARE sites, §2's 2+58 category-A/C sites) —
expected, since `Vector3`/`Point3` in particular are the idiomatic spelling for any geometric
quantity (origins, axes, directions) across the whole mechanical/kinematic/FEA stdlib surface,
not a narrow corner case. **Disposition: EXCLUDED-BY-DESIGN (quantity-slot territory, §12) for
every one of the 118** — recorded here solely so 5627 candidate-2's follow-up task has a sized
starting point (9 stdlib files, 4 example/fixture files, 21 Rust test files), not because any
of them needs migration under β/γ/δ₁/δ₂.

```
$ git diff --exit-code -- crates/ examples/ gui/ stdlib
$ echo $?
0
```

No source was read via a build for this step — every figure above is a static `git grep` +
direct-read classification; there is nothing to revert.

## §9 Item 6 — Load-struct intersection flag for PRD 5, §10.2 (step-9)

**Purpose: FLAG, then EXCLUDE**, so this PRD's (γ/β) migration list and PRD 5's
(`dimension-checked-readers.md`, landed `efba5a8036` — confirmed a real commit at HEAD)
90-site list are **provably disjoint**: no site is double-counted or double-edited by two
different tasks (§10.2's filing rule).

### 9.1 The four `Real`-typed load fields, re-verified at HEAD

| Field | Cited at | Text at HEAD |
|---|---|---|
| `PointLoad.force` | `crates/reify-compiler/stdlib/fea_multi_case.ri:315-317` | `structure def PointLoad : Load {` / `param point : String = ""` / `param force : Real = 0.0` |
| `PressureLoad.magnitude` | `:418-419` | `structure def PressureLoad : Load {` / `param magnitude : Real = 0.0` |
| `TractionLoad.traction` | `:446-448` | `structure def TractionLoad : Load {` / `param face : String = ""` / `param traction : Real   = 0.0` |
| `BodyForce.force_density` | `:476-478` | `structure def BodyForce : Load {` / `param body : String = ""` / `param force_density : Real   = 0.0` |

All four **CONFIRMED still `Real` at HEAD `08c6c42be9`** — line numbers re-verified by
direct `grep -n` against the file, exactly matching the governing PRD's own §10.2 citation
(same four spans). **`Gravity.magnitude : Acceleration = STANDARD_GRAVITY()` (`:516`) is the
ONLY dimensioned Scalar field in the entire FEA-load cluster today** — also re-verified
verbatim.

### 9.2 Full enumeration of the FEA-load cluster (audit surface for the exclusion)

Every `structure def … : Load` in the tracked corpus (`git grep -n "structure def.*: Load\b" -- '*.ri'`
confirms `fea_multi_case.ri` is this trait's *only* implementor repo-wide):

| Structure | Fields | Dimensioned-Scalar? |
|---|---|---|
| `PointLoad` (`:315`) | `point : String`, `force : Real` | No — `Real` is dimensionless |
| `PressureLoad` (`:418`) | `magnitude : Real`, `face : Option<FaceSelector>`, `direction : String` | No |
| `TractionLoad` (`:446`) | `face : String`, `traction : Real` | No |
| `BodyForce` (`:476`) | `body : String`, `force_density : Real` | No |
| `Gravity` (`:515`) | `magnitude : Acceleration`, `direction : List<Real>` | **Yes** — `Acceleration` is a named dimension already in the §0.1 name set |

(Recorded for completeness, not because it is in scope: `FixedSupport`/`PinnedSupport`
(`:354`/`:379`) implement the sibling `Support` trait, not `Load`, and each carries only
`target : String` — no Scalar field at all. Excluded on two independent grounds and not
enumerated further.)

**`Gravity` is the one FEA-load-cluster structure this PRD's γ already reaches** —
`Gravity.magnitude` is dimensioned `Scalar` today, so it was never outside the family γ
promotes; it needs no exclusion because it was never in the "flag-then-exclude" set to
begin with. It is also, empirically, a non-issue for β: a repo-wide `git grep -n "Gravity("`
over every tracked `.ri`/`.rs` file finds every call site passing either no `magnitude`
override (bare `Gravity()`, the default) or a compound expression (`STANDARD_GRAVITY()`,
`5 * STANDARD_GRAVITY()`, `2 * STANDARD_GRAVITY()`, `0 * STANDARD_GRAVITY()`) — **zero bare
numeric literals**, so there is no BARE `Gravity.magnitude` call site anywhere in the corpus
today for β to migrate. (This is a direct-grep spot-check, not a full walker-equivalent
sweep — offered as corroborating context for this section, not as a new item; it does not
change any other section's count.)

### 9.3 Polarity finding, re-verified empirically (not merely re-quoted from the PRD)

The governing PRD's §10.2 states the seam is *"the opposite of the obvious reading"*:
because `Real` is dimensionless `Scalar`, and dimensionless `Scalar` is already in the
vetted family (§0 anchor table #1) **regardless of α's flip**, the units-**correct** call is
the one that warns today, and the units-*wrong* (bare) call is silent. Re-derived
independently this session — not trusted from the PRD's prose — with the built
`target/debug/reify check` binary against a two-file scratch repro
(`/tmp/5756-scratch/step9-pointload-{polarity,dimensioned}.ri`, uncommitted, outside the
tracked tree, modelled on the existing `PointLoad(point: "tip", force: 1000.0)` call in
`crates/reify-eval-fea-tests/tests/fixtures/fea_no_supports.ri:19`):

```
$ ./target/debug/reify check /tmp/5756-scratch/step9-pointload-polarity.ri      # force: 1000.0 (bare)
warning: W_MODULE_DECL_MISSING: file has no top-of-file `module` declaration; ...
All constraints satisfied.
$ echo $?
0

$ ./target/debug/reify check /tmp/5756-scratch/step9-pointload-dimensioned.ri   # force: 1000N (units-correct)
warning: argument 'force' has type 'Scalar[m·kg·s^-2]' but param 'force' requires type 'Real'
warning: W_MODULE_DECL_MISSING: file has no top-of-file `module` declaration; ...
All constraints satisfied.
$ echo $?
0
```

**Confirmed exactly as PRD §10.2 states:** `PointLoad(force: 1000.0)` (bare, units-wrong) is
silent; `PointLoad(force: 1000N)` (units-correct) already warns, TODAY, at current HEAD,
with **no predicate flip involved on either side** — the flip only widens
`general_leaf_param_family_is_validated`'s *dimensioned*-`Scalar` arm (§0 anchor table #1);
the *dimensionless* arm that governs `Real` params was already `true` before this task
touched anything, so this diagnostic is identical whether or not α's local flip is applied
(re-confirmed: the flip was not applied for either invocation above). This polarity is the
**opposite** of every other item in this ledger (there, a bare literal at a *dimensioned*
param is the thing γ newly catches; here, a bare literal at this *dimensionless* param is
already the silent/clean case, and it is the *dimensioned* value that already warns) — which
is exactly why γ's promotion changes nothing here: it has no dimensionless-arm effect to
give, and the dimensioned-arm effect it does have cannot reach a field that isn't dimensioned
yet.

### 9.4 Disjointness proof and PRD-5 citation

Because all four fields (§9.1) are `Real` — outside the dimensioned family — at HEAD, and
because γ's flip only ever *adds* eligibility to the dimensioned-`Scalar` arm without
touching the dimensionless arm's existing behaviour (§9.3), **every call site of
`PointLoad`/`PressureLoad`/`TractionLoad`/`BodyForce` is outside this PRD's (γ/β) migration
list by construction, today and after γ lands** — there is no diagnostic these four fields
could newly produce under γ that they do not already produce today. Their existing
diagnostic behaviour changes only at the moment PRD 5 retypes a field to a dimensioned
`Scalar`, which is PRD 5's act, not this task's or β's.

**PRD 5 = `dimension-checked-readers.md`** (landed `efba5a8036`, per this task's own
governing PRD's §10.2 citation) **owns the retyping and its own 90-site call-site
migration**: 90 in-scope constructor sites (78 `PointLoad` + 2 `TractionLoad` + the
remainder), split 35 `.ri` / 77 `.rs` — **PRD 5's own measured figure, cited here, not
re-derived**; re-deriving it would duplicate PRD 5's work and risk silently drifting from
its own count the moment that count moves. The governing PRD records a **reciprocal check,
already passed**, from PRD 5's own text: *"PRD 4 [this program] owns `conformance/mod.rs` /
`type_compat.rs` …; this PRD [5] owns the `.ri` retype + the 90-site migration"* — no
reciprocal "the other owns it" ambiguity between the two PRDs.

**Filing discipline (§10.2, honoured here):**
- **No PRD-5-owned work is filed from this task.** The 90 call sites are PRD 5's to
  enumerate, not this ledger's — re-listing them here would risk a stale duplicate the
  moment PRD 5's own count moves.
- **No δ leaf is filed for the `CTOR_FIELD_CONFORMANCE_SEVERITY` Warning→Error flip.** Per
  the governing PRD's own ownership clarification (§10.2, D4-7/§12), that flip is a
  separate, pre-existing work item this PRD's decomposition neither owns nor blocks on;
  PRD 5's seam row phrasing could be misread as assigning it to "PRD 4" (this program) but
  does not actually do so — flagged here so a later reader does not mis-file it against this
  ledger's consumers.

### 9.5 Revert proof

This step read source only (`fea_multi_case.ri`, the governing PRD) and ran the pre-built
`target/debug/reify` binary against two uncommitted scratch fixtures under `/tmp/`; it
required no local predicate flip and touched no tracked file:

```
$ git status --porcelain
$ git diff --exit-code -- crates/ examples/ gui/ stdlib
$ echo $?
0
```

## §10 Per-site classified ledger + consumer index (step-10)

**Provenance continuation.** Measured at post-step-9 HEAD **`ffca87cd60`** (2026-07-30), base
`main` = `bae556d6ad43` (the rebase noted at session start, `783715ca` → `bae556d6ad43`, 6 files:
`crates/reify-eval/{src/dirty.rs,src/engine_edit.rs,src/engine_fixpoint.rs,tests/harness_engine.rs,
tests/harness_engine/flat_sort_kahn_core_delegation.rs,tests/unified_dag_edit_path.rs}` — **zero
overlap**, confirmed by direct inspection of that file list, with every file this ledger's counts
depend on: `conformance/mod.rs`, `type_compat.rs`, `dimension.rs`, `examples/**`, `stdlib/**`,
`gui/test/fixtures/**`, `tree-sitter-reify/**`, `examples_smoke.rs`,
`struct_ctor_field_conformance_tests.rs`, `input_shape_eval_e2e.rs`,
`auto_type_param_determinism_tests.rs` — so §§0-9's figures need no re-verification). Also
re-verified fresh this session (needed by §11, used here): the two `gui/test/fixtures` reach-vs-gate
anchors — `gui/src-tauri/src/debug_server.rs:1236` (`"large_assembly" =>
Some("gui/test/fixtures/large_assembly.ri".to_string()),`) and `gui/test/visual/assertions.ts:47`
(`large_assembly: "gui/test/fixtures/large_assembly.ri",`) — both **CONFIRMED present, unchanged**;
neither file appears in any of the four rebases' changed-file lists.

**This section performs no new measurement.** It is a synthesis of §§1-9's already-measured,
already-cited data into one indexed table, adding four columns none of §§1-9 individually carries
(disposition, owning consumer, and a severity-at-HEAD/post-migration transition) and a
cross-section reconciliation. Every row cites the subsection it was lifted from; nothing below is
re-derived from scratch.

### 10.0 Table method, compression discipline, and scope

**Row granularity.** Each source section already chose a row granularity when it first measured a
bucket (e.g. §3.1 groups `large_assembly.ri`'s 6 field-hits into 2 rows; §2.2.4 groups the 4
`param_default_type_mismatch_tests.rs`/`let_annotation_type_mismatch_tests.rs` pin sites into 4
row-groups covering 10 literal sites). This table **reproduces each source table at its own
established granularity**, adding columns rather than re-splitting rows — re-exploding to one row
per literal value would not add information and would risk a transcription error on every one of
~160 rows. A `#` column records how many individual sites each row represents, so no count is
lost to the grouping.

**Severity is stated once per gate subsection, not per row**, because it is constant within a gate
(every hit at a given gate shares the same HEAD-vs-post-migration transition) — repeating it on
~160 rows would be pure duplication. The step's column spec names this "severity at HEAD
(Warning/Error) and post-γ"; that literal phrasing is accurate for gates 1/2/6 (γ's own blast
radius) but **not** for gate "3+4" (owned by δ₁, which γ never touches — confirmed §5.0/§2's own
gate-3+4-vs-gate-2 distinction) or gate 5 (owned by δ₂, a third, independent mechanism — §5.0).
Each subsection below states its own correct transition and owning leaf explicitly rather than
mechanically labeling every row "post-γ," which would misstate gates 3+4 and 5's actual mechanism.

**Two out-of-scope items are represented as rollups, not exploded rows**, because their own
sections already ruled on this and re-deriving finer granularity here would re-litigate a settled
call: (a) §8's 118 quantity-slot sites — §8.2 itself declines per-site classification for the
72-site Rust-fixture bucket ("the number exists solely to size the follow-up... not individually
re-classified"); (b) §9's PRD-5 90-site load-struct list — §9.4's filing discipline explicitly
forbids re-enumerating it here ("No PRD-5-owned work is filed from this task... re-listing them
here would risk a stale duplicate"). Both appear in §10.6 as counts-with-pointers, outside the
6-column schema, consistent with their own sections' rulings.

**Class↔gate-5 mapping, stated once:** the step's CLASS enum (`BARE, CROSS-DIMENSION, NON-SCALAR,
SCALARPARAM-FALSE-POSITIVE`) has no literal "INT-FOR-LENGTH" slot; gate 5's two sub-counts (§5.3
cross-dimension, §5.4 int-for-length) map onto it as: §5.3 → `CROSS-DIMENSION` (a `Scalar{Q}` at a
`Scalar{R}` slot), §5.4 → `BARE` (a bare `Int` is the integer-flavored case of "an undimensioned
value at a dimensioned slot," the same phenomenon §§1-4 call BARE for `Real` literals).

**Same file:line, two gates.** `bearing_auto_seal.ri:46` and `mv-2-priv-param.ri:4` are each hit by
*two* independent mechanisms (gate 2 = `conformance/mod.rs`, γ's own blast radius; gate "3+4" =
`entity.rs`, δ₁'s) — they appear once under each gate below, correctly, because "gate" is part of
this table's row key and the two mechanisms can (and here do) disagree on disposition for the
identical site (§4.2's central finding).

### 10.1 Gate 1 — ctor field-slot (`conformance/mod.rs`'s ctor-call check; owner **γ**)

Severity for every row: silent at HEAD → `Warning` (`CTOR_FIELD_CONFORMANCE_SEVERITY`) once γ
lands, unless noted. Source: §3.1 (BARE), §3.2 (EXCLUDED).

| File:line | Text | Bucket | Class | # | Disposition | Owning consumer |
|---|---|---|---|---|---|---|
| `examples/trajectory/tots_optimal_ptp.ri:67,78,79` | `JointLimit.max_force` / `TOTSShaper.velocity_limit`/`.acceleration_limit` | examples/** | BARE | 3 | BREAKS — migrate | **β**, ahead of γ |
| `examples/trajectory/printer_print_envelope.ri:153,154,155` | same 3 fields | examples/** | BARE | 3 | BREAKS — migrate | **β**, ahead of γ |
| `gui/test/fixtures/large_assembly.ri:18,27,36` | `Material.density` | other .ri | BARE | 3 | BREAKS — migrate | **β** (no cargo gate behind this file — §11(A); a GUI e2e run or `reify check`, not a cargo test, must confirm the fix) |
| `gui/test/fixtures/large_assembly.ri:19,28,37` | `Material.youngs_modulus` | other .ri | BARE | 3 | BREAKS — migrate | **β**, same caveat |
| `crates/reify-compiler/tests/struct_ctor_field_conformance_tests.rs:1420` | `Limit(velocity_limit: 300.0, acceleration_limit: 5000.0)` | Rust fixture | BARE | 2 | **DELIBERATE NEGATIVE TEST — invert** (`excluded_family_dimensioned_scalar_given_dimensionless_real_is_silent`, §1.3) | **γ** (inverts/removes its own pinning test as part of landing) |
| `crates/reify-eval/tests/input_shape_eval_e2e.rs:248,255,256` | `JointLimit.max_force`, `TOTSShaper.velocity_limit`/`.acceleration_limit` | Rust fixture | BARE | 3 | BREAKS — migrate, **silent** (suite stays GREEN under the flip — §1.4, only inspects `Severity::Error`) | none forcing — γ may optionally migrate as part of its own PR; not required to land safely |
| `crates/reify-syntax/tests/harness_syntax/auto_binding_sites_lowering_tests.rs:245,360` | `Bearing(bore: 1.0)` inside an undeclared-structure parse fixture | Rust fixture | BARE | 2 | EXCLUDED — parse-only (walker never runs) | none — `reify-syntax` never depends on `reify-compiler` |
| `…function_call_named_args_tests.rs:128` | `Steel(density: 1000.0)` inside an undeclared-structure parse fixture | Rust fixture | BARE | 1 | EXCLUDED — parse-only | none |
| `crates/reify-cli/tests/fixtures/stdlib_sim_ready_material_ok.ri:11` | `Material(density: 7850.0)` against a file-local dimensionless `Material` shadow | other .ri | BARE | 1 | EXCLUDED — file-local dimensionless shadow | none — not a real dimensioned-family site |
| `purpose_compile_tests.rs:1814`, `purpose_activation.rs:1547` | same `Material(density: 7850.0)` shadow pattern | Rust fixture | BARE | 2 | EXCLUDED — file-local dimensionless shadow | none |
| `termination_check_tests.rs:78,346` | `Inner(x: 5)` against a file-local dimensionless `Inner` shadow | Rust fixture | BARE | 2 | EXCLUDED — file-local dimensionless shadow | none |

**Sub-totals:** BARE = 17 (BREAKS 15, DELIBERATE-INVERT 2) + EXCLUDED = 8 (parse-only 3,
file-local-shadow 5) → **25 rows**. By bucket: examples/** 6 · other .ri 7 (6 BREAKS + 1 EXCLUDED)
· Rust fixture 12 (5 BARE + 7 EXCLUDED) · stdlib 0. CROSS-DIMENSION = 0, NON-SCALAR = 0 for this
gate (§3.4 — corroborated over the corpus slice the four step-1 suites exercise, not exhaustively
re-proven over the full workspace).

### 10.2 Gate 2 — param-default entry (`conformance/mod.rs`; owner **γ**)

Severity for every row: silent at HEAD → `Warning` once γ lands. `param`-only (the `let` twin is
gate "3+4", §2.2.5b). Source: §2.1 (category A), §2.2 (category C), §4.1/§4.2 (gate-2-specific
disposition).

| File:line | Text | Bucket | Class | # | Disposition | Owning consumer |
|---|---|---|---|---|---|---|
| `examples/bearing_auto_seal.ri:46` | `param durometer : Length = 70.0` | examples/** | BARE | 1 | BREAKS — migrate, **empirically confirmed** (§1.2's transcript) | **β** — same fix as its gate-"3+4" row below; β fixes the *annotation* (Shore durometer is dimensionless), not the literal |
| `tree-sitter-reify/test/fixtures/mv-2-priv-param.ri:4` | `priv param rated_torque : Torque = 5` | other .ri | BARE | 1 | EXCLUDED — parse-only (tree-sitter grammar fixture, never reaches `reify-compiler`) | none |
| Category-C `param` sites, c1 group (§2.2.4, 15 file:line rows, all `param`) | e.g. `grade : Length = 8.8`, `axis: Length = 0`, `material : Length = 1.0` … | Rust fixture | BARE | 15 | BREAKS — migrate, **silent under gate 2** (§4.2: none of these additionally break because of gate 2 — their currently-passing assertions filter to `Severity::Error` only) | none forcing — γ may optionally migrate |
| Category-C `param` sites, c2 group (`param_default_type_mismatch_tests.rs:175-178,206-207`) | `zero_int=0, one_int=1, half_real=0.5, large_real=70.0, neg_real=-5.0, neg_int=-1` | Rust fixture | BARE | 6 | BREAKS — migrate, **silent under gate 2** (§4.2: these assert on `ParamDefaultTypeMismatch`, a different code from gate 2's `ArgTypeMismatch` — invisible to gate 2, though they are gate-"3+4" pins, see §10.3) | none forcing under gate 2 |
| Category-C `param` sites, c0 group — **actionable** (§2.2.4, 9 row-groups; **12 CONFIRMED + 11 PRESUMED**, §2.2.4's evidence roll-up) | e.g. `engine_tests.rs` structural-only reads (8, PRESUMED), `let_scope_tests.rs` message-content filters (3, CONFIRMED), … | Rust fixture | BARE | 23 | BREAKS — migrate, **silent under gate 2** | none forcing |
| Category-C `param` sites, c0 group — **parse-only** (`reify-syntax/src/ts_parser.rs:6338,6364,6420,6450,6466,6477,6488,6499`) | in-crate `parse()` unit-test fixtures | Rust fixture | BARE | 8 | **EXCLUDED — parse-only** (all 8 enclosing `#[cfg(test)]` tests call only `parse`/`ts_parser.parse`/`lower_port_body_directly`, never a compiler entry point, so gate 2 can never run on them — §2.2.4b, same ruling as the `mv-2-priv-param.ri:4` row above) | none |
| Category-C `param` sites, unclassified (§2.2.4, `trait_assoc_type_conformance_tests.rs:76,224` — 2 occurrences not re-read this session) | — | Rust fixture | BARE | 2 | **UNCLASSIFIED** — provisionally BREAKS — migrate pending resolution | δ₁/γ (whichever lands first) must resolve definitively before landing, not this ledger |

**Sub-totals:** BARE = 56 (1 empirically-confirmed BREAKS + 46 silent-BREAKS [15+6+23+2] + 9
EXCLUDED-parse-only [1 `mv-2-priv-param.ri` + 8 `ts_parser.rs`]) → **7 rows** (rollup granularity;
56 underlying sites). By bucket: examples/** 1 · other .ri 1 · Rust fixture 54 · stdlib 0. Matches
§4.1's "47 candidates" (56 raw minus the 9 parse-only exclusions) exactly.

### 10.3 Gates 3+4 — δ₁'s entity.rs annotation checks (`entity.rs:479-485`/`:563-569`; owner **δ₁**)

Severity for every row: silent at HEAD (existing numeric-leniency carve-out, mirroring gate 5's
Rule 4) → **`Error`** once δ₁ removes that carve-out (gates 3/4 are the primary type checker, not
a Warning-only secondary walker — unlike gates 1/2/6). δ₁ is a wholly separate mechanism from γ;
γ landing does not change any row in this subsection (§2's own title: "gates 3+4, δ₁'s blast
radius"). Source: §2.1 (categories A/B), §2.2.4 (category C c1/c2/c0), §2.3 (EXCLUDED-BY-DESIGN).

| File:line | Text | Bucket | Class | # | Disposition | Owning consumer |
|---|---|---|---|---|---|---|
| `examples/bearing_auto_seal.ri:46` | `param durometer : Length = 70.0` | examples/** | BARE | 1 | **BREAKS — migrate, MANDATORY before δ₁ lands** (post-δ₁ this is a hard compile `Error` in a *shipped example* — inferred from the mechanism, not empirically rebuilt under a not-yet-built δ₁; flagged as such) | **β**, same fix as its gate-2 row above |
| `tree-sitter-reify/test/fixtures/mv-2-priv-param.ri:4` | `priv param rated_torque : Torque = 5` | other .ri | BARE | 1 | EXCLUDED — parse-only (never reaches `entity.rs` either) | none |
| Category B (`let …`) | — | — | — | 0 | n/a — **0 sites found** (§2.1; 35 non-bare `let`-dimensioned sites surveyed, all compound expressions) | n/a |
| Category-C, c1 (§2.2.4, 15 rows, reused verbatim from §10.2's table — same 15 sites; includes `trait_assoc_type_conformance_tests.rs:110` override + `:166` inherited-default) | `grade : Length = 8.8`, `axis: Length = 0`, … | Rust fixture | BARE | 15 | **BREAKS — migrate** (breaks a currently-passing `errors_only`/`parse_and_compile`-style assertion today once δ₁ lands) | **δ₁** (migrates its own test suite as part of landing) |
| Category-C, c2 — `param_default_type_mismatch_tests.rs:175-178,206-207` | 6 sites (`zero_int`, `one_int`, `half_real`, `large_real`, `neg_real`, `neg_int`) | Rust fixture | BARE | 6 | **DELIBERATE NEGATIVE TEST — invert** (asserts `ParamDefaultTypeMismatch` is absent; δ₁ inverts) | **δ₁** |
| Category-C, c2 — `let_annotation_type_mismatch_tests.rs:176-178,583` | 4 sites (`x=5,y=0.5,z=-5.0,d=5`) | Rust fixture | BARE | 4 | **DELIBERATE NEGATIVE TEST — invert** | **δ₁** |
| Category-C, c0 — **actionable** (§2.2.4, 9 row-groups, reused from §10.2; **12 CONFIRMED + 11 PRESUMED**) | — | Rust fixture | BARE | 23 | BREAKS — migrate, lower-urgency (no currently-passing test forces it today). **The 11 PRESUMED rows (`helpers.rs:1077,1078` · `purpose_compile_tests.rs:1567` · `engine_tests.rs`×8) rest on a structural inference, not a read of the enclosing assertion — δ₁ must read them before landing a change that depends on the disposition** | **δ₁**, optional cleanup as part of landing |
| Category-C, c0 — **parse-only** (`reify-syntax/src/ts_parser.rs:6338,6364,6420,6450,6466,6477,6488,6499`) | in-crate `parse()` unit-test fixtures | Rust fixture | BARE | 8 | **EXCLUDED — parse-only** (never reaches `entity.rs` either; all 8 enclosing tests call only a parser entry point, never a compiler one — §2.2.4b, same ruling as the `mv-2-priv-param.ri:4` row above) | none — **not δ₁ cleanup work; migrating them provably cannot change any diagnostic** |
| Category-C, unclassified (`trait_assoc_type_conformance_tests.rs:76,224`) | — | Rust fixture | BARE | 2 | UNCLASSIFIED — provisionally BREAKS — migrate | **δ₁** must resolve before landing |
| `crates/reify-compiler/stdlib/units.ri:14-19,23-24,28-31,35-37,41-43,48,53-54,65-66,74` (24 lines) | `pub unit cm : Length = 0.01`, … (§2.3's full 24-line table) | stdlib .ri | BARE-shaped | 24 | **EXCLUDED-BY-DESIGN (C2 unit decls)** — required bare conversion factors, never reach gate 3/4 (`unit`, not `param`/`let`) | **none — δ₁ must NEVER touch these** |
| `examples/m9_combined.ri:46`, `examples/integration_full_v01.ri:33` | `unit mil : Length = 0.0000254` (×2) | examples/** | BARE-shaped | 2 | EXCLUDED-BY-DESIGN (C2 unit decls) | none — must never be touched |

**Sub-totals:** BARE = 2 (category A) + 58 (category C: 15+10+23+8+2) = 60 → **BREAKS 39** (1
bearing + 15 c1 + 23 c0-actionable), **DELIBERATE-INVERT 10** (c2), **UNCLASSIFIED 2**,
**EXCLUDED-parse-only 9** (1 `mv-2-priv-param.ri` + 8 `ts_parser.rs`) — check: 39 + 10 + 2 + 9 = 60.
EXCLUDED-BY-DESIGN = 26 (24 stdlib + 2 examples/**). **Total 86 rows/sites** across 10 row-groups
(60 BARE + 26 EXCLUDED-BY-DESIGN).
By bucket: examples/** 3 (1 BREAKS + 2 EXCLUDED-BY-DESIGN) · stdlib 24 (EXCLUDED-BY-DESIGN) ·
other .ri 1 (EXCLUDED-parse-only) · Rust fixture 58 (check: 3 + 24 + 1 + 58 = 86).
Of the 39 BREAKS, **28 are CONFIRMED by a read of the enclosing assertion** (1 bearing + 15 c1 +
12 c0-actionable-CONFIRMED) and **11 are PRESUMED** (§2.2.4's evidence roll-up).

### 10.4 Gate 5 — constraint-def Rule 4 (`type_compat.rs:1482-1488`; owner **δ₂**)

Severity for every row: silent-tolerated (Rule 4 returns `true`) at HEAD → **`Error`** once δ₂
removes Rule 4 (§5.2, `Diagnostic::error` at the rejection fallthrough — Rule 4 is Error-severity
by construction, unlike gates 1/2/6's Warning). δ₂ is a third, independent mechanism — step-1's
transcript contains no gate-5 evidence at all (§5.0); this gate's own instrumented sweep
(§5.1-§5.4) is its evidence base. Source: §5.3 (cross-dimension), §5.4 (int-for-length).

| File:line | Text | Bucket | Class | # | Disposition | Owning consumer |
|---|---|---|---|---|---|---|
| `type_compat.rs:5041` (`constraint_arg_type_conforms_mass_for_length_is_true`) | param=`Scalar{Length}`, arg=`Scalar{Mass}` (direct `Type` values, no `.ri` source) | Rust fixture (primitive pin) | CROSS-DIMENSION | 1 | DELIBERATE NEGATIVE TEST — invert | **δ₂** |
| `type_compat.rs:5050` (`..._dimensionless_for_length_is_true`) | param=`Scalar{Length}`, arg=`Scalar{dimensionless}` | Rust fixture (primitive pin) | CROSS-DIMENSION | 1 | DELIBERATE NEGATIVE TEST — invert | **δ₂** |
| `type_compat.rs:5031` (`..._int_for_length_is_true`) | param=`Scalar{Length}`, arg=`Int` | Rust fixture (primitive pin) | BARE (int-for-length) | 1 | DELIBERATE NEGATIVE TEST — invert | **δ₂** |
| `constraint_def_compile_tests.rs:1454` (`int_literal_for_length_param_no_constraint_arg_type_mismatch`) | `constraint def MinWall { param w: Length … }` / `constraint MinWall(w: 3)` | Rust fixture | BARE (int-for-length) | 1 | DELIBERATE NEGATIVE TEST — invert (test's own body: "numeric leniency — dimensional strictness is task 4490's job") | **δ₂** |
| `forall_statement_lower_tests.rs:1646` (`forall_constraint_inst_body_emits_per_element_inst_predicates`) | `constraint def MinThreshold {…}` / `forall v in [1,2,3]: constraint MinThreshold(value: v)` | Rust fixture | BARE (int-for-length) | 1 (3 firings — one per `forall` element) | **BREAKS — migrate** (its own `errors_only(&module).is_empty()` assertion would newly fail; fix is migrating the fixture's literals to unit-bearing ones, e.g. `[1mm,2mm,3mm]`, not inverting — its *purpose* is forall label mechanics, not gate-5 tolerance) | **δ₂** (migrates its own test fixture as part of landing) |

**Sub-totals:** CROSS-DIMENSION = 2 (both DELIBERATE-INVERT, 0 corpus sites — §5.3). BARE
(int-for-length) = 3 sites / 5 firings (2 DELIBERATE-INVERT + 1 BREAKS — §5.4). **5 rows**, all
Rust-fixture bucket (0 examples/**, stdlib, other-.ri — §5.1.2's full-corpus instrumented sweep
found zero `.ri`-shaped hits of either shape). Two structural prerequisites carried forward, not
resolved here (δ₂ must carry its own fence, not this ledger): §5.5 (`Type::ScalarParam` admission
into `is_numeric`, 0 corpus sites today but latent) and §5.6 (Rule 2 precedes Rule 4 — a
generic/trait-typed constraint-def *param* never reaches Rule 4; no site here was excluded by
Rule 2, but a later reader must check it before attributing a silence to Rule 4).

### 10.5 Gate 6 — fn-call entry (`entities_phase.rs:1493`/`:1503`; owner **γ**) — 0 rows

**Measured count: 0** (§7.2's repo-wide census — no `fn` param both `type_carries_trait_object`
and lockstep-recursing to a bare dimensioned `Type::Scalar` leaf). Reported per §7.3/§0 design
decision as a **measurement at this HEAD, re-checkable, not a structural guarantee**: §7.1
demonstrates the entry is mechanically *reachable* (the `couple_opt`/`takes` demos both fire, one
at exit 1) — the 0 is a fact about today's corpus content, not the guard's shape. Per the plan's
own escalation clause: since the count is 0, **no β-ordering change and no escalation are filed**;
if this ledger is ever cited against a later HEAD, §7.2's census must be re-run first.

**SCALARPARAM-FALSE-POSITIVE class (item 3, §6) — 0 rows, γ-scoped implementation prerequisite,
not a migration item.** §6.1's census of all 7 dimension-generic `fn` definition sites found zero
that forward a `Scalar<Q>` into a concrete dimensioned ctor field/param default — so this class has
**no real corpus row** in this table. It is recorded here only as a pointer: **γ must wire
`is_numeric_placeholder_leaf` (or an equivalent fence) into the general concrete-leaf arm before
promoting the predicate**, or any *future* dimension-generic forwarding fn will newly false-positive
(§6.2's scratch probe demonstrates this live, reversibly, off-corpus). The two probes in §6.2/§6.3
are scratch-only mechanism demonstrations, not corpus sites, and are deliberately not rows here.

### 10.6 Out-of-scope items, carried for completeness (not part of the migration list)

| Item | Count | Disposition | Pointer |
|---|---|---|---|
| Quantity-slot residual (`Vector3`/`Point3`/`Matrix`/`Tensor`/`Field`) | 118 (34 stdlib/9 files + 8 examples/4 files + 4 other-.ri/3 files + 72 Rust-fixture/21 files) | EXCLUDED-BY-DESIGN — dimension-blind by construction (task 5465's shape-based arms, `is_numeric_placeholder_leaf`-gated); γ's predicate promotion never reaches them | §8 — sizing input for the 5627 candidate-2 / `reify-core/src/ty.rs` follow-up only |
| FEA load-struct fields (`PointLoad.force`, `PressureLoad.magnitude`, `TractionLoad.traction`, `BodyForce.force_density`) | 4 field-definitions; **0 call sites** (all still `Real` at HEAD, outside the dimensioned family — no BARE call site exists for γ to catch) | EXCLUDED — PRD-5 load struct (flag-then-exclude, §9.4's disjointness proof) | §9 — PRD 5 (`dimension-checked-readers.md`, landed `efba5a8036`) owns the retype + its own 90-site call-site migration; **not re-enumerated here** per §9.4's filing discipline |

### 10.7 Reconciliation against step-1's empirical evidence

Two independent empirical sources exist; neither is conflated with the other or with this table's
mechanically-implied (not rebuilt-and-run) rows.

**(a) The four-suite flipped run (§1), 9 diagnostics observed, zero residual:**
- `examples_smoke`: 7 diagnostics = 6 gate-1 BARE (`tots_optimal_ptp.ri`×3 + `printer_print_envelope.ri`×3,
  §10.1) + 1 gate-2 BARE (`bearing_auto_seal.ri`, §10.2). 6 + 1 = 7. ✓.
- `struct_ctor_field_conformance_tests`: 1 failing test, 2 diagnostics = the gate-1
  `struct_ctor_field_conformance_tests.rs:1420` DELIBERATE-INVERT row (§10.1), both args. ✓.
- `input_shape_eval_e2e` / `auto_type_param_determinism_tests`: 0 diagnostics *observed* (both
  GREEN, §1.4) — consistent with §10.1's row for `input_shape_eval_e2e.rs`'s 3 gate-1 BARE sites
  being marked "silent," not with those sites being absent.
- **7 + 2 = 9 total empirically-triggered diagnostics, all 9 accounted for by rows in §10.1/§10.2.
  Zero unexplained residual.**

**(b) The independent gate-5 instrumented sweep (§5), 7 raw `RULE4_HIT` firings, self-reconciled
in §5.4.1 and reproduced as §10.4's 5 rows/8 firings** (2+1 primitive pins + 1 site/1 firing
`constraint_def_compile_tests.rs` + 1 site/3 firings `forall_statement_lower_tests.rs` = 2+1+1+3 =
7 firings across 5 sites — matches §5.4.1 exactly). This sweep is structurally separate from (a):
step-1's transcript contains no gate-5 evidence (§5.0), so (a)'s 9 and (b)'s 7 are never summed
against each other.

**Every other row in §§10.1-10.4 (BARE/CROSS-DIMENSION sites not in (a) or (b), and every EXCLUDED
row) is either (i) confirmed present by direct source read but not exercised by any suite step-1
or §5 actually ran (e.g. gate-1's `large_assembly.ri` 6 sites — no suite loads that file, §3.3; or
gate-2/gate-"3+4"'s 54/58 Rust-fixture sites — step-1 never compiled `param_default_type_mismatch_tests.rs`
etc. under the flip), and is labeled "mechanical, not empirically re-confirmed this session"
wherever that applies (§4.1's own phrase, carried forward here); or (ii) an EXCLUDED row, whose
whole point is that no gate ever fires there.** No row in this table claims empirical confirmation
it does not have.

### 10.8 Consumer index

| Consumer | Reads (table rows) | Reads (section counts) | Must NOT touch |
|---|---|---|---|
| **β** (corpus migration, lands before γ/δ₁) | §10.1's examples/**+other-.ri BREAKS rows (12 sites: 6 `tots`/`printer` + 6 `large_assembly`) · §10.2's `bearing_auto_seal.ri` row · §10.3's `bearing_auto_seal.ri` row (same fix, dual benefit) | §3.1's correct-twin hint (`examples/large_assembly.ri:51-53`) · §3.3's reach-vs-gate note (large_assembly has no cargo gate — verify via GUI harness/`reify check`, not `cargo test`) | §10.3/§2.3's 26 EXCLUDED-BY-DESIGN `unit` sites (β must not "fix" required conversion factors) · §10.1/§10.2's EXCLUDED file-local-shadow/parse-only rows (not real sites) |
| **γ** (predicate promotion) | §10.1 + §10.2 in full (its own blast radius) · §10.5 (gate 6, 0 sites — safe to land without also touching fn-call sites) | §6 in full (the `is_numeric_placeholder_leaf` fence is a **landing prerequisite**, not optional — §6.2's live demonstration) · §7 (fn-call reachability is mechanical, re-check §7.2's census before citing "0" at a later HEAD) · examples_smoke's own panic prose (reuse item: "γ — not α — rewrites its panic prose") | §10.3 (gates 3+4 — a different, δ₁-owned mechanism γ never touches) · §10.4 (gate 5 — δ₂-owned) · §10.6's quantity-slot residual (out of scope by design, §12) |
| **δ₁** (param/let default tolerance removal, gates 3+4) | §10.3 in full: c1 (15, migrates own test suite), c2 (10, inverts own pins), c0-actionable (**23**, optional — of which **11 are PRESUMED** and must be read before δ₁ relies on their disposition), unclassified (2, `trait_assoc_type_conformance_tests.rs:76,224` — must resolve before landing) | §2.2.4's full per-site c1/c0 detail incl. its CONFIRMED/PRESUMED evidence column (this table's rollup rows point back to it) · §2.2.5's `param`/`let` split ruling | §10.3's 26 EXCLUDED-BY-DESIGN `unit` sites (explicit warning, §2.3: "δ₁ 'fixing' them would silently break the unit system") · §10.3's **8 EXCLUDED-parse-only `ts_parser.rs` sites** (§2.2.4b — not cleanup work; migrating them provably cannot change any diagnostic) |
| **δ₂** (constraint-def numeric leniency removal, gate 5) | §10.4 in full (5 rows: 3 primitive pins + `constraint_def_compile_tests.rs` pin + `forall_statement_lower_tests.rs` BREAKS site) | §5.5 (ScalarParam admission into `is_numeric` is a δ₂ **prerequisite**, unresolved here — a future dimension-generic constraint arg needs its own fence) · §5.6 (Rule 2 precedes Rule 4 — check before attributing a silence to Rule 4) | §10.1-§10.3 (gates 1/2/3+4/6 — different mechanisms, different files) |
| **§6.4 quantity-slot follow-up** (5627 candidate-2 / `reify-core/src/ty.rs`) | §10.6's quantity-slot rollup row only | §8 in full (118 sites, 9+4+3+21 files, the worked `kinematic.ri`/`dynamics.ri` contrast in §8.1) | Everything else — this residual is explicitly not a migration list for β/γ/δ₁/δ₂ (§8, §12) |

### 10.9 Citation contract

Every count any later leaf (β, γ, δ₁, δ₂, the §6.4 follow-up, or any future task in this program)
asserts about the dimensioned-construction corpus must cite **this ledger, at the HEAD SHA under
which the citing count was (re-)confirmed** — never PRD §6's scoping-phase figures directly, and
never this task's own decompose-time plan `analysis`/`design_decisions` text, on faith.

This is a **correction** of the plan's own framing, not a restatement of it. The plan's
decompose-time `analysis` asserted that PRD §6.2's "51 named dimensions" was "already stale
against HEAD's 49" (repeated in this task's `design_decisions` block). §0.1 above re-derived the
figure three independent ways and found the **opposite**: **51 is CONFIRMED accurate at HEAD**,
and the plan's own "49" was **not reproducible** — traced to a regex bug (a
whitespace-sensitive `\(DimensionVector::` pattern silently drops the two entries whose tuples
rustfmt wraps across 4 lines) this session caught reproducing live, plus a second candidate
origin — a *different question* being answered (distinct `DimensionVector` **values** vs "51
spellable names"), which collapses the 51 into the 48-50 band. Per §0.1's caveat the neighbouring
note's specific "49 vectors" is quoted, not re-derived, and does not reconcile against §0.1's own
three documented collisions (which give 48); the two candidate origins for the plan's figure are
**not distinguishable from the available evidence**, and neither is claimed as "the" source.
Reported non-blocking, `esc-5756-1`, before §0 was written.

The lesson is therefore **not** "prefer 49 over 51," nor "PRD prose is always stale" — in this
instance the PRD prose was right and this task's own plan text was wrong. The lesson is the one
the plan states correctly elsewhere ("re-CONFIRM against HEAD rather than re-derive from PRD
figures"): **anchors are point-in-time (this ledger's own header caveat), and every figure must be
re-verified at whatever HEAD it is being cited from, regardless of which prior document — PRD or
this task's own plan — it happens to agree or disagree with.** Cite this ledger's §0.1 for the
dimension-name-set ruling (51 names, 65 with the alias union) and §§1-9/§10 for every downstream
migration count; do not cite PRD §6 prose or this task's plan `analysis` block as ground truth for
either without independently re-confirming it first.

**10.9.1 Correction changelog.** Because this section makes the figures above NORMATIVE for
β/γ/δ₁/δ₂, every post-publication correction is logged here. A consumer who already read an
earlier revision can diff their citations against this list.

| Date | Commit | Correction | Direction | Root cause |
|---|---|---|---|---|
| 2026-07-30 | `2b02ef0e5e` (step-12) | §2.3 `pub unit` declarations in `stdlib/units.ri` **21 → 24**; derived EXCLUDED-BY-DESIGN total (§2.3, §10.3, §10.8 β/δ₁ rows) **23 → 26** | **GREW** — the do-not-touch list got *larger* | The prose total counted only "6 base + 15 bare factor" and silently dropped the three EXPRESSION/OFFSET-form declarations (`deg`:24, `degC`:42, `degF`:43) that §2.3's own verbatim listing always showed. Correct decomposition: 6 + 15 + 3 = 24. |
| 2026-07-30 | `e242ba6f83` (step-13) | §2.2 category C **55 → 58** (c0 **28 → 31**; `param`/`let` split **51/4 → 54/4**), threaded through §4.1 (52 → 55 gate-2 candidates), §5, §8.3, §10.2, §10.3, §10.7, §10.8 | **GREW** — 3 previously-invisible sites recovered | The documented comment-stripper was **string-literal-blind**: it cut the line at the first `//` even inside a Rust string literal, so any fixture whose embedded `.ri` snippet *opened* with a doc/line comment had its payload discarded before the regex ran. §2.2.1 now documents the corrected string-aware stripper. |
| 2026-07-30 | amendment pass (§2.2.4b) | The **8 `reify-syntax/src/ts_parser.rs` c0 sites reclassified BREAKS → EXCLUDED — parse-only.** Derived figures restated: §4.1 gate-2 candidates **55 → 47** (56 raw − **9** parse-only, was − 1); §10.2 sub-totals "54 silent-BREAKS + 1 EXCLUDED-parse-only" → "**46** silent-BREAKS + **9** EXCLUDED-parse-only"; §10.3 "BREAKS 47" → "**BREAKS 39**, EXCLUDED-parse-only **9**" (39+10+2+9 = 60); §10.8's δ₁ row c0 **31 → 23 actionable**. Category-C total (58), BARE (60), and §10.3's Total (86) are **unchanged** — only dispositions moved. | **SHRANK** — the normative BREAKS/migration counts got *smaller*; no new work anywhere | The ledger classified two structurally identical parse-only situations oppositely: `mv-2-priv-param.ri:4` was `EXCLUDED — parse-only` while the 8 `ts_parser.rs` sites, unreachable by the same *kind* of argument (each enclosing `#[cfg(test)]` test calls only a parser entry point — re-verified at HEAD, §2.2.4b), were folded into c0's flat "BREAKS — migrate". §10.10.2's addend audit ticked the sums ✓ because it re-checked the arithmetic without re-checking the classification the arithmetic rested on. **NB:** as first published, this row justified the reclassification from the dependency graph; that justification was FALSE and is retracted by the row below. The reclassification itself, and every figure in this row, survived re-verification unchanged. |
| 2026-07-30 | amendment pass (§2.2.4) | **CONFIRMED/PRESUMED evidence marker added to the c0 rows** — 31 c0 = **20 CONFIRMED / 11 PRESUMED** (`helpers.rs:1077,1078` · `purpose_compile_tests.rs:1567` · `engine_tests.rs`×8); of the 23 c0-actionable sites, **12 CONFIRMED / 11 PRESUMED**. §10.3 now also reports the BREAKS split (**28 CONFIRMED / 11 PRESUMED**) and §10.8 tells δ₁ which rows still need a read. | **NEUTRAL** — no count changed; confidence made legible | §2.2.4 already marked 11 dispositions "presumed", but §10.2/§10.3 rendered them with the same authority as the rows whose enclosing assertion was actually read. A c0 disposition turns on whether the enclosing test filters to `Severity::Error` (§1.4) — a property of code that was not read for those 11. |
| 2026-07-30 | amendment pass (§2.2.4/§10.3 c1) | **`file:line` anchors added for the 2 prose-cited c1 rows** — `trait_assoc_type_conformance_tests.rs:110` (override) and `:166` (inherited-default); the 2 UNCLASSIFIED occurrences in the same file pinned as `:76` and `:224`. | **NEUTRAL** — no count changed | Two of the 15 highest-urgency c1 rows were cited by sub-case name only, the sole exception to §2.2.5's "individually re-verifiable, file:line by file:line" promise — and in the one file that also holds the 2 unresolved occurrences, so a consumer could not tell which were already accounted for. |
| 2026-07-30 | amendment pass (§0.4) | **§0.4's published reproduction command corrected** to scope the parse-only exclusion to `.rs` files (`grep -vE '^crates/reify-(syntax\|ast)/tests/.*\.rs$'`); it now reproduces the stated union of **2120**. | **NEUTRAL** — substrate count unchanged; the *command* was wrong, not the figure | The published `grep -v '^crates/reify-syntax/tests/\|^crates/reify-ast/tests/'` applied to the whole union, additionally dropping 2 `.ri` fixtures that tree B's 168 counts — so the command emitted 2118 at the cited HEAD, not the tabulated 2120. Neither dropped file holds an in-scope site (§0.4). |
| 2026-07-30 | amendment pass (§11(B)) | **Check 2's command corrected** to the range form `git diff --exit-code main...HEAD -- crates/ examples/ gui/` (vacuous `stdlib` pathspec dropped). | **NEUTRAL** — the invariant held and still holds; the *proof* is now sound | The old check 2 was a working-tree diff ("nothing uncommitted is dirty"), not a branch-vs-`main` diff, so it could not have caught a *committed* source edit; and the `stdlib` pathspec matched 0 tracked files (`stdlib/` lives at `crates/reify-compiler/stdlib/`). |
| 2026-07-30 | amendment pass (§2.2.4b justification) | **§2.2.4b's dependency-graph justification RETRACTED and replaced by per-test-body evidence.** The 8 `ts_parser.rs` rows' evidence marker changes `CONFIRMED (deps read)` → `CONFIRMED (test bodies read)`, with per-site `fn`/call anchors (`fn` at `:6333,6358,6416,6446,6465,6476,6487,6498`); §2.2.4's claim that the Cargo.toml read was "stronger … settles the whole file at once" is deleted and explicitly withdrawn; §4.1's and §10.2/§10.3's restatements of the argument are reworded. **DISPOSITION AND EVERY COUNT UNCHANGED** — all 8 remain `EXCLUDED — parse-only`; §4.1's 47, §10.2's 46+9, §10.3's 39, §10.8's 23, category-C 58, BARE 60, §10.3's Total 86 all stand. | **NEUTRAL** — nothing to re-plan; only the *reason* beside the figures changed | The published justification rested on a FALSE premise: it read `crates/reify-syntax/Cargo.toml`, saw no `reify-compiler` on either dep list, and concluded gate 2 was unreachable "by any path". But `[dev-dependencies]` lists `reify-test-support`, and `reify-test-support/Cargo.toml:14` carries `reify-compiler` as a **normal** dep — dev-deps are linked into in-crate `#[cfg(test)]` tests, so `reify-compiler` IS transitively available to `ts_parser.rs`'s test module, which already calls `reify_test_support::bracket_source()` at `:4525`. Same failure mode as §10.10 was written to close (arithmetic audited, underlying classification argument not re-derived) — reintroduced one level up *by the fix for it*, this time in the justification rather than the sums. The disposition survived only because the per-body ground was independently true. |
| 2026-07-30 | amendment pass (§0.1/§11.A.4) | **§0.1's secondary "49 distinct `DimensionVector` values" aside downgraded from asserted fact to unreconciled hearsay.** It was quoted from `docs/notes/units-gating-gap-research-2026-07-28.md:308` and never re-derived, and it contradicts §0.1's *own* evidence: the three value-collisions that section documents (`Stiffness`/`TranslationalStiffness`, `AbsorptionCoeff`/`Curvature`, `Impulse`/`Momentum`) deduct from 51 to **48**, not 49. Newly verified and recorded instead: 51 tuples, and **50** distinct `DimensionVector` *constant identifiers* (only `IMPULSE` is spelled twice). §11.A.4's matching "most likely is the actual source" claim softened — the regex bug and the distinct-vector conflation are **not distinguishable** as origins of the plan's 49. | **NEUTRAL** — no sweep input and no count in §§2-9 touches any distinct-*vector* figure; the ruled 51 is unaffected | Same failure mode as the §2.2.4b justification retraction one row above: a neighbouring document's number was repeated with the authority of a measurement, and its arithmetic was never checked against the evidence sitting beside it in the same paragraph. Caught while verifying `esc-5756-1`. The load-bearing figure was re-confirmed in passing — the ledger's full 51-name list is **set-identical** to a fresh extraction from `dimension.rs:514-595` at this branch's HEAD (`dimension.rs` is unchanged vs `main`, so the `08c6c42be9` attribution still holds). |
| 2026-07-30 | amendment pass (§12) | **Appendix §12 added**: the normative scanner (self-contained — rebuilds its file lists from §0.4's `git ls-files` walk, embeds the 65-name set, reads no `/tmp`), and the full **60-row `file:line` hit list** with per-row class + evidence marker, inlined into this document. | **NEUTRAL** — no count changed; the normative count made re-derivable from the committed artifact alone | §2.2.1 declared its stripper normative and obliged consumers to reproduce **58**, but the only executable form was uncommitted `/tmp/5756-scratch/` scratch (hard-coding a second uncommitted input and six tree-list files), and the 58 sites were never listed individually (31 c0 sites were rolled into 11 prose row-groups, several without line numbers). `/tmp` does not survive a reboot or a warm-lane reclaim, and β/γ/δ₁/δ₂ land later by construction. |
| 2026-07-30 | amendment pass (§2.2, §2.2.5, §12.2) | **File-count basis disambiguated: "58 in-scope hits across 17 files" → 58 hits / *15* files in-scope, 60 hits / *17* files raw.** | **NEUTRAL** — no hit count changed | Surfaced by §12's per-row list: `let_type_disambiguation_tests.rs` and `m9_error_cases.rs` hold exactly one hit each and both are §2.2.3 exclusions, so the 60→58 step also drops 2 whole files. The old sentence paired an in-scope hit count with a raw file count. §2.2.5's "short by 5 files" is 17-vs-22 and is unaffected. |

**No migration disposition changed under the first two corrections.** The `pub unit` sites were
already EXCLUDED-BY-DESIGN and remain so — three more of them, not a new class. The three recovered
category-C sites are all the same structurally-parse-only **c0** class as the already-accounted
`ts_parser.rs:6488` (§2.2.4). Nothing moved into or out of c1/c2; no BREAKS row and no
DELIBERATE-INVERT row was added or removed. **β/γ/δ₁/δ₂ must not re-plan off either correction** —
only counts moved, and in both cases the *safety* list (do-not-touch / no-break) is what grew.

**The amendment-pass corrections likewise require no re-planning.** The one that moves a
disposition (§2.2.4b) moves 8 sites *out* of the migration list, and every one of them was
provably incapable of changing a diagnostic — so a consumer holding the pre-amendment figures has
over-scoped work, never under-scoped it. Nothing moved into or out of c1/c2 there either. The
remaining amendment rows change no count at all: they add evidence markers, line anchors, a
corrected reproduction command, a corrected proof command, and the appendix. **Direction summary
for a consumer diffing citations: `pub unit` GREW (21 → 24, do-not-touch list), category C GREW
(55 → 58, all EXCLUDED-parse-only), BREAKS SHRANK (§4.1 55 → 47, §10.3 47 → 39).**

### 10.10 Post-review arithmetic reconciliation (step-14)

The two corrections above thread through six sections, so the failure mode this subsection exists
to close is a **half-applied correction** leaving the ledger internally inconsistent — which is
exactly the defect class the review caught in the first place.

**10.10.1 Residual-figure sweep.** Every superseded figure was re-grepped across the whole ledger
in a *count* sense (`21, 23, 28, 44, 51, 52, 53, 55, 57, 80`), re-derived rather than trusted from
the review's own line checklist. **No superseded count survives.** Every surviving occurrence is
one of the following legitimate, deliberately-unchanged uses:

| Surviving occurrence | Why it is not a stale count |
|---|---|
| §2.2 header "PRD figure: 63 … (25 c1 + 10 c2 + **28** c0, 11 of the 28 parse-only)" | The **PRD's own** scoping figure, quoted as the comparand. Correcting it would erase the delta §2.2.5 exists to state. |
| §2.2.1/§2.2.2/§2.2.4/§2.3 correction narrative ("**55** → 58", "**57** → 60", "an earlier revision said **21**", "c0 moves **28** → 31") | Deliberate before/after prose and verbatim transcript output. |
| §0.1, §0.2, §0.3, §10.9, §11.A.4 — the **51**-name `NAMED_DIMENSIONS` ruling | A different, already-resolved question (the 49/51/34 discrepancy), untouched by this review. |
| §8's "72 sites across **21** files", §8.3's "**21** Rust test files", §10.8's "9+4+3+**21** files" | The quantity-slot census's own file count. Different method (repo-wide `git grep`), different question. |
| §5's "**21** targeted files" / "0 hits through shard 21" | The `reify-eval` partial-run caveat. Unrelated. |
| File:line anchors — `units.ri` listing rows `23`/`28`/`53`, `large_assembly.ri:51-53` and its `sed` recipe, `type_compat.rs:52-281`, `ports.ri`/`constitutive.ri:54-57`, `dimensionless_unification.ri:50-51`, `constraint_inst_tests.rs:57` | Positions in source files, not counts. |
| §11's provenance HEAD `80f877d7cc` (the leading `80`), §5's "**23** files" git-range note | A commit SHA and a provenance file count. |

**10.10.2 Addend audit.** Every stated sum in the corrected sections was re-computed; where a
sentence and a total disagreed, the *sentence* was fixed, never the total silently:

| Section | Stated sum | Checks |
|---|---|---|
| §2.3 | 6 base + 15 bare factor + 3 expression/offset = **24**; 24 stdlib + 2 example-local = **26** | ✓ ✓ |
| §2.2 | 15 c1 + 10 c2 + 31 c0 + 2 unclassified = **58**; 54 `param` + 4 `let` = **58** | ✓ ✓ |
| §2.2.5 | 63 (PRD) − 58 (measured) = short by **5** | ✓ |
| §4.1 | 54 (cat C) + 2 (cat A) = **56**; 56 − **9** parse-only = **47** | ✓ ✓ |
| §10.2 | 1 confirmed BREAKS + **46** silent-BREAKS [15+6+23+2] + **9** EXCLUDED-parse-only = **56**; matches §4.1's 47 candidates (56 − 9) | ✓ ✓ |
| §10.3 | BARE 2 + 58 = **60**; category C 15 + 10 + **23** + **8** + 2 = **58**; BREAKS 1 + 15 + **23** = **39**; 39 + 10 + 2 + **9** = **60**; 60 + 26 = **86**; bucket 3 + 24 + 1 + 58 = **86** | ✓ ✓ ✓ ✓ ✓ ✓ |
| §2.2.4 (evidence) | c0 **20** CONFIRMED + **11** PRESUMED = **31**; c0-actionable **12** + **11** = **23**; 23 + 8 parse-only = **31** | ✓ ✓ ✓ |
| §10.3 (evidence) | BREAKS **28** CONFIRMED [1 + 15 + 12] + **11** PRESUMED = **39** | ✓ |

**[amendment pass] The §4.1/§10.2/§10.3 rows above were restated by §2.2.4b's parse-only
correction, and the two evidence rows are new.** This is the failure mode §10.10 exists to close,
caught one level up: the pre-amendment audit ticked `54 + 2 = 56` and `56 − 1 = 55` ✓✓ because it
re-checked the *arithmetic* without re-checking the **classification the arithmetic rested on**.
An addend audit is only as strong as the row labels it sums — where a sum is derived from a
disposition (`… − N parse-only`), the disposition must be re-derived too, not carried forward.

**10.10.3 The review comment's own totals do not add — resolved here so no future reader
reconciles against the review instead of against this ledger.** Both review findings independently
proposed "**Total 80 → 83**" for §10.3. That is arithmetically inconsistent *as a joint
recommendation*: the two corrections are independent and **compose** — 80 + 3 (`pub unit`
21 → 24) + 3 (category C 55 → 58) = **86**. **83 is only the intermediate value** after the
`pub unit` fix alone — which is precisely why the two fixes were landed as separate commits
(`2b02ef0e5e` leaves the ledger internally consistent at 83; `e242ba6f83` takes it to 86).
**86 is the figure to cite.** Any consumer holding an 80 or an 83 is reading a pre- or
mid-correction revision.

**10.10.4 [amendment pass] Residual-figure sweep for the parse-only correction.** The
superseded gate-2/BREAKS figures (`55` candidates, `54` silent-BREAKS, `47` BREAKS, `31` c0
*as a disposition group*, `1` parse-only exclusion) were re-grepped across the whole ledger.
Every surviving occurrence is one of the following legitimate uses:

| Surviving occurrence | Why it is not a stale count |
|---|---|
| §2.2's header/narrative "**55** → 58", "an earlier revision reported 55", §10.10.3's "category C 55 → 58" | Deliberate before/after correction prose for a *different*, already-logged correction. |
| §4.1's "(**55** → 47)", §10.9.1's "**55 → 47**", §10.10.2's "`56 − 1 = 55` ✓✓" post-mortem | The amendment's own before/after prose. |
| c0 = **31 sites** (§2.2.4 header, §2.2.4's "28 → 31", §5's "the c0 bucket (31 sites)", §10.10.2's evidence row) | c0 is **still 31 sites** — the correction split their *disposition* (23 actionable + 8 parse-only), not their membership. Any "31" describing c0 *size* is current; only "31" used as a **BREAKS** count was restated. |
| **54** `param` / §2.2.5b, §10.2's "Rust fixture 54" bucket line, §10.10.2's "54 (cat C) + 2 = 56" | The category-C `param` **bucket** count, unchanged: the 8 parse-only sites are still Rust-fixture category-C `param` hits, still inside the 54/56/58. |
| `trait_assoc_type_conformance_tests.rs:**31**`, `units.ri` listing rows `31`/`54`, `assertions.ts:**47**`, `ports.ri`/`constitutive.ri:**54**-57` | File:line anchors, not counts. |
| §10.3's "**BREAKS 47** → 39" is fully restated; **no un-restated `47` remains as a BREAKS count** | — |

**Invariants preserved across the amendment:** category-C total **58**, `param`/`let` split
**54/4**, §10.2's BARE **56**, §10.3's BARE **60**, EXCLUDED-BY-DESIGN **26**, §10.3 Total **86**,
and every by-bucket line. The correction is disposition-only; it moves no site between buckets and
adds or removes no site from any total.

## §11 Methodology closure + no-source-change proof (step-11)

**Provenance.** Measured at HEAD **`80f877d7cc`** (step-10's own commit; no further rebase landed
between step-10 and this section), base `main` = `bae556d6ad43` — same base as §10's provenance
note (zero-overlap with this ledger's dependency files already established there and in §6's).

### 11(A) Reach census (addendum C3)

Every count below was re-run mechanically this session (`git ls-files`, HEAD `80f877d7cc`), not
carried forward from the plan's decompose-time text on faith — and it reconciles to that text
exactly (zero drift across all five rebases since decompose time, because none of them touched a
`.ri` file — confirmed by inspecting all four rebases' changed-file lists, §10's provenance note
and §6's).

**11.A.1 What the reused walk (`corpus_no_bare_scalar.rs`, pre-2/§0.4) reaches: 438 of 595 tracked `.ri` files.**

| Tree | Pattern | Count | Reached? |
|---|---|---|---|
| `examples/**` | `examples/*.ri` | 258 | YES |
| `crates/**` | `crates/*.ri` | 168 | YES |
| `gui/test/**` | `gui/test/*.ri` | 12 | YES — but see 11.A.3, reach ≠ gate |
| **Reached subtotal** | | **438** | |

**11.A.2 Out of its reach entirely, and compiled by no gate: 157 tracked `.ri` files.**

| Tree | Count | Why unreached |
|---|---|---|
| `docs/prds/**/fixtures/**` | 50 | Not one of the walk's five trees (§0.4); PRD-authoring fixtures |
| `tests/prd-gate/fixtures/**` | 71 | Not one of the walk's five trees; **many are orphaned** — spot-checked 5 of 71 fixture names against every `.rs`/`.json` file outside their own directory: 4 of 5 matched at least one reference (a `*-probe-set.json` or a `.rs` test), 1 of 5 (`collection_sub_at_placement_rejected.ri`) matched **zero** files outside `tests/prd-gate/fixtures/` itself — corroborating, not exhaustively re-proving, the "many orphaned" characterization (a full 71-file audit is outside this step's scope). The directory also holds exactly 7 probe-set `.json` files and 1 `README.md` at its top level, confirmed present by direct listing. |
| `designs/litter_tray/**` | 7 | Not one of the walk's five trees; a worked design example, not test/example corpus |
| `prj/**` | 2 | Not one of the walk's five trees (also separately excluded from §0.2's alias census — `prj/printer_v01/printer.ri:60`'s local `Acceleration` redeclaration) |
| `tree-sitter-reify/test/fixtures/**` | 27 | Not one of the walk's five trees; parse-only via the tree-sitter grammar test harness, never reaches `reify-compiler` — the same parse-only mechanism as §0.4/§3.2's individual exclusions, just an entire directory of it |
| **Unreached subtotal** | **157** | |

**438 + 157 = 595 — exact reconciliation, no residual, no double-count**, matching the plan's own
decompose-time figures precisely (unlike the `NAMED_DIMENSIONS` cardinality, §11.A.4, nothing here
needed correction). Separately: this task's own worktree — an isolated warm-lane checkout, not the
shared dev checkout CLAUDE.md's own conventions describe — has **zero** `.orchestrator-scratch/`,
`.claude/worktrees/`, or `.eval-worktrees/` directories today (checked directly, `[ -d ... ]`).
That is a fact about *this* checkout, not what makes the counts trustworthy: pre-2 built the
exclusion discipline into the *method* (every file list here is driven from `git ls-files`, never
a raw filesystem walk), so the counts reproduce identically regardless of which checkout they are
run from. (Addendum C3's own "167" figure counted untracked scratch copies present in a
*different* checkout at a *different* time — §0's premise-verification note, carried forward here
rather than re-litigated.)

**11.A.3 The reach-vs-gate distinction (why "corpus clean" ≠ "every `.ri` clean").** The 438 files
the walk reaches are not uniformly *compiled* by a gate either — most sharply for the 12
`gui/test/fixtures/*.ri` files (including `large_assembly.ri`, §10.1's 6-row BREAKS target):
swept by the tree walk (so counted in every §§2-9 sweep and in §10's table), but loaded only by
`gui/src-tauri/src/debug_server.rs:1236` (`"large_assembly" =>
Some("gui/test/fixtures/large_assembly.ri".to_string()),`) and
`gui/test/visual/assertions.ts:47` (`large_assembly: "gui/test/fixtures/large_assembly.ri",`) —
both **re-confirmed present, unchanged, this session** (also noted in §10's provenance note).
Neither is a `cargo test` target. **Consequence for β:** a passing `cargo test -p reify-compiler`
(or any other crate) proves nothing about whether `large_assembly.ri`'s BARE sites are fixed —
only the GUI e2e harness (`gui/test/visual/*`) or a manual
`reify check gui/test/fixtures/large_assembly.ri` can confirm it. The remaining 426 of the 438
reached files (`examples/**` + `crates/**`) sit behind at least one cargo gate each —
`examples/**` via `examples_smoke` (§0 anchor #9) or a targeted eval test, `crates/**` via
whichever suite embeds or loads that fixture — so this specific gap is confined to the
`gui/test/fixtures/**` tree, not general across all 438 reached files.

**11.A.4 `NAMED_DIMENSIONS` 49/51/34 discrepancy — ruling carried forward, not re-litigated.**
§0.1 resolved this: the array holds **51** names at HEAD (three independent re-derivations, all
51), the plan's own decompose-time "49" was **not reproducible** (a regex bug, reported
`esc-5756-1`), and the `dimension.rs:513` doc comment's "34" is stale/contradicted. Every count in
§§2-9 uses the 51-name registry set unioned with the 14 `.ri` type aliases from §0.2 (65 names
total, §0.3) — never the plan's "49" and never the doc comment's "34". §10.9's citation contract
states the consequence for later leaves normatively.

### 11(B) No-source-change proof

Re-run fresh this session, at HEAD `80f877d7cc` (post-step-10), base `main` = `bae556d6ad43`:

```
$ git diff main...HEAD --name-only
docs/notes/dimensioned-construction-blast-radius-2026-07-29.md

$ git diff --exit-code main...HEAD -- crates/ examples/ gui/
$ echo $?
0

$ git status --porcelain
(empty — no output)
```

**Three independent checks, all clean:**

1. **`git diff main...HEAD --name-only` names exactly one path, and it is under `docs/notes/`** —
   the entire task diff, across all 11 steps and 2 prerequisites, is the single ledger file this
   section is part of. No `-transcript.md` sibling was ever created (step-1's transcript stayed
   under the ~200-line inline threshold; the design decision permitted either outcome, and this is
   the one that happened, recorded here as fact rather than left implicit).
2. **`git diff --exit-code main...HEAD -- crates/ examples/ gui/` exits 0** — every local flip this
   task applied (step-1's predicate flip, §5's Rule-4 `eprintln!` instrumentation, every probe file
   referenced in §§6-9) was reverted before that step's own commit, and no revert was ever
   incomplete going into the next step. This is not new evidence — §1.5, §5.7, §6.4, §7.4, and
   §9.5 each already proved their own step's revert at the time it happened; this is the
   **cumulative** proof, run once more now that all 11 steps are committed, that no revert
   regressed a later one and nothing was left dirty at the end.

   **[post-review] Two defects in this check's earlier spelling, both fixed above.** (a) It read
   `git diff --exit-code -- crates/ examples/ gui/ stdlib`. The `stdlib` pathspec matches **nothing**
   — there is no top-level `stdlib/` in this repo (it lives at `crates/reify-compiler/stdlib/`, and
   `git ls-files -- 'stdlib/*'` returns 0 files). `git diff` does not error on an unmatched
   pathspec, so that component was **vacuous** while reading as if it independently covered stdlib;
   the real coverage came incidentally from `crates/`. Dropped, since `crates/` already covers it.
   (b) A bare `git diff --exit-code -- <paths>` compares the **working tree** to the index/HEAD, not
   HEAD to `main` — it proves "nothing uncommitted is dirty", which is check 3's job, not "this
   branch changed no source relative to `main`". A branch that had *committed* a source edit would
   still have exited 0. Re-spelled in the `main...HEAD` **range** form, which is what makes checks 1
   and 2 **mutually corroborating** — check 1 enumerates the changed paths and check 2 independently
   asserts the source trees are byte-identical to `main` — rather than check 2 being a weaker
   restatement of check 3. Before this fix the whole claim rested on check 1 alone, unflagged.
3. **`git status --porcelain` is empty** — no untracked scratch file, probe, or build artifact
   leaked into the tracked working tree at any point. Every scratch artifact this task used
   (`/tmp/5756-scratch/**`, including the `/tmp/5756-scratch/probes/*.ri` files §§4/6/7/9 built)
   lived outside the repository entirely, per each step's own scratch-location note; `target/`
   (gitignored) was rebuilt several times from flipped and clean source across steps 1/4/6 but is
   correctly absent from both this diff and this status output.

**11(B).1 Re-run after the three post-review commits (step-14).** The corrections of §10.9.1 added
three further commits (`2b02ef0e5e`, `e242ba6f83`, and this one) after the proof above was
written. Re-run at HEAD `e242ba6f83`, base `main` = `bae556d6ad`, with only this subsection's own
edit to the same ledger file pending:

```
$ git diff main...HEAD --name-only
docs/notes/dimensioned-construction-blast-radius-2026-07-29.md

$ git diff --exit-code main...HEAD -- crates/ examples/ gui/; echo $?
0

$ git status --porcelain
(empty — no output)
```

**Unchanged on all three checks.** The three review-fix commits are pure ledger edits: the
`main...HEAD` name list is still the single `docs/notes/` path, `crates/ examples/ gui/` is
still byte-identical to `main`, and no scratch artifact leaked in. The corrections re-ran the
category-C sweep from `/tmp/5756-scratch/cat_c_scan4.py` — outside the repository, like every
other measurement this task made — and touched no source file to do it. **The invariant that α
lands nothing in source holds across all 14 steps.**

**11(B).2 Re-run after the amendment-pass commits.** Re-run at HEAD `19e38761ef`, base
`main` = `bae556d6ad43`, using the corrected range-form check 2 (this subsection's own edit to the
same ledger file is the one pending modification `git status` reports):

```
$ git diff main...HEAD --name-only
docs/notes/dimensioned-construction-blast-radius-2026-07-29.md

$ git diff --exit-code main...HEAD -- crates/ examples/ gui/; echo $?
0

$ git status --porcelain
 M docs/notes/dimensioned-construction-blast-radius-2026-07-29.md
```

**Unchanged, and now proved by a check that could have failed.** The amendment commits are pure
ledger edits, and check 2 in its range form asserts what the section always claimed: the
`crates/`, `examples/` and `gui/` trees on `task/5756` are byte-identical to `main` across every
commit on the branch, not merely undirty in the working tree. **The invariant that α lands nothing
in source holds across all 14 steps plus the amendment pass.** No verification step was skipped to
reach that statement: the ledger is a docs-only artifact under `docs/notes/`, so the merge gate's
Rust/GUI blocks have nothing in this diff to compile — the applicable checks are exactly the three
above, all re-run at this HEAD.

**This is the auditable evidence for the task's hard invariant.** α measured the blast radius of
promoting `general_leaf_param_family_is_validated`'s dimensioned-`Scalar` arm across all six
gates that reach it or a sibling numeric-leniency mechanism, produced the per-site migration
ledger §10 indexes for β/γ/δ₁/δ₂ and the §6.4 quantity-slot follow-up to consume, and **landed
nothing under `crates/`, `examples/`, `gui/`, or `stdlib`** — the entire task diff is the `files`
list this plan declares (in the end, one of the two declared paths; the second was never needed,
per §1's design decision allowing either outcome).


## §12 Appendix — self-contained re-derivation of §2.2's normative 58 (amendment pass)

§2.2.1 declares its stripper **normative** and §10.9 obliges β/γ/δ₁/δ₂ to reproduce **58**. Until
this appendix, the only *executable* form of that method was
`/tmp/5756-scratch/cat_c_scan4.py` — uncommitted scratch that hard-coded a second uncommitted
input (`names-alternation.txt`) and six tree-list files passed as `argv`. This note references
`/tmp/5756-scratch/**` artifacts throughout as substantiating evidence; every one of them was
present and re-run during the amendment pass, but **`/tmp` does not survive a reboot or a warm-lane
reclaim, and β/γ/δ₁/δ₂ land later by construction.** The regex and the stripper body were already
inlined (§2.2.1), but the 58 sites themselves were not: §2.2.4 gives the 15 c1 rows individually
and rolls the 31 c0 sites into 11 prose row-groups, several without line numbers — so a consumer
who could not run the script could not re-derive the count they are obliged to reproduce.

This appendix closes that gap **inside the committed artifact**. Nothing below reads `/tmp`.

### 12.1 The scanner, self-contained

Depends on nothing but the repository and Python 3. It rebuilds tree C + tree D with §0.4's own
`git ls-files` walk (`.rs`-scoped exclusion — §0.4's post-review correction), embeds §0.3's 65-name
search set, and applies §2.2.1's regex and string-literal-aware stripper verbatim. Run from the
repository root; it exits non-zero if the in-scope count is not 58.

```python
#!/usr/bin/env python3
"""Self-contained re-derivation of the ledger's normative category-C count (58).

No inputs beyond the repo itself: builds tree C + tree D with the same
`git ls-files` walk §0.4 publishes, embeds §0.3's 65-name search set, and
applies §2.2.1's string-literal-aware comment stripper and regex verbatim.

Run from the repository root:  python3 cat_c_scan.py
Expected at the ledger's HEAD:  RAW=60  IN_SCOPE=58  FILES=17
"""
import re, subprocess, sys

# --- §0.3 search set: 51 NAMED_DIMENSIONS names + 14 `.ri` type aliases ------
NAMES = """
AbsorbedDose AbsorptionCoeff Acceleration Action AmountOfSubstance Angle
AngularVelocity Area ArealCostRate Capacitance Charge Conductance Current
Curvature Density DielectricStrength DynamicViscosity ElectricalConductivity
ElectricResistivity Energy Force ForceDensity FractureToughness Frequency
HeatCapacity HeatFlux Illuminance Impulse Inductance InverseAmount Jerk
Length LuminousFlux LuminousIntensity MagneticFlux MagneticFluxDensity Mass
MolarGasConstant MomentOfInertia Momentum Money Permeability Permittivity
Power Pressure Resistance RotationalDamping RotationalStiffness SolidAngle
SpecificHeat StefanBoltzmannDim Stiffness Stress Temperature
ThermalConductivity ThermalExpansion ThermalResistance Time Torque
TranslationalDamping TranslationalStiffness Velocity Voltage Volume
VolumetricFlowRate
""".split()
assert len(NAMES) == 65, len(NAMES)

# Longest-first so `ThermalConductivity` wins over a `Thermal…` prefix.
ALT = '|'.join(re.escape(n) for n in sorted(NAMES, key=len, reverse=True))

# --- §2.2.1 predicate --------------------------------------------------------
PAT = re.compile(
    r'(?:priv\s+)?(?:param|let)\s+[A-Za-z_][A-Za-z0-9_]*\s*:\s*(?:' + ALT + r')\b'
    r'\s*=\s*-?[0-9]+(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?(?![a-zA-Z_0-9.])'
    r'(?=\s*(?:,|\)|;|\}|//|"|\\|$))'
)

# --- §2.2.3 exclusions: trait-requirement `let`s, a different mechanism ------
EXCLUDED = {
    'crates/reify-compiler/tests/harness_langcore/let_type_disambiguation_tests.rs:296',
    'crates/reify-compiler/tests/m9_error_cases.rs:276',
}


def strip_trailing_comment(line):
    """`//` starts a comment only OUTSIDE a Rust string literal (§2.2.2 (3))."""
    i, n, in_string = 0, len(line), False
    while i < n:
        c = line[i]
        if in_string:
            if c == '\\':
                i += 2
                continue
            if c == '"':
                in_string = False
            i += 1
            continue
        if c == '"':
            in_string = True
            i += 1
            continue
        if c == '/' and i + 1 < n and line[i + 1] == '/':
            if i == 0 or line[i - 1] != ':':     # original `:` guard, preserved
                return line[:i]
        i += 1
    return line


def trees_c_and_d():
    """§0.4's walk, .rs half: crates/**/*.rs (minus self + the two parse-only
    test dirs) + gui/src-tauri/**/*.rs."""
    out = subprocess.run(
        ['git', 'ls-files', '--', 'crates/*.rs', 'gui/src-tauri/*.rs'],
        capture_output=True, text=True, check=True).stdout.split()
    drop = re.compile(r'corpus_no_bare_scalar\.rs$|^crates/reify-(syntax|ast)/tests/.*\.rs$')
    return sorted(f for f in out if not drop.search(f))


def main():
    files = trees_c_and_d()
    hits = []
    for rel in files:
        with open(rel, encoding='utf-8', errors='replace') as fh:
            for lineno, raw in enumerate(fh, start=1):
                line = raw.rstrip('\n')
                if line.strip().startswith('//'):      # whole-line comment
                    continue
                for _ in PAT.finditer(strip_trailing_comment(line)):
                    hits.append((f'{rel}:{lineno}', line.strip()))
    in_scope = [h for h in hits if h[0] not in EXCLUDED]
    print(f'TREE_C+D_FILES={len(files)}')
    print(f'RAW={len(hits)}  IN_SCOPE={len(in_scope)}  '
          f'FILES={len({k.rsplit(":", 1)[0] for k, _ in in_scope})}')
    for k, text in hits:
        print(f'{"EXCL " if k in EXCLUDED else "     "}{k}: {text}')
    return 0 if len(in_scope) == 58 else 1


if __name__ == '__main__':
    sys.exit(main())
```

Measured during the amendment pass, at this branch's HEAD:

```
$ python3 cat_c_scan.py | head -2
TREE_C+D_FILES=1683
RAW=60  IN_SCOPE=58  FILES=15
```

`TREE_C+D_FILES=1683` vs §0.4's `1641 + 41 = 1682`: **one file was added to `crates/**/*.rs`
between §0.4's measurement HEAD (`08c6c42be9`) and this branch's rebased base** — the same
+1 drift §0.4's corrected command shows (2120 at `08c6c42be9`, 2121 now). The 60 raw hits are
unchanged across that drift.

### 12.2 File-count basis — 17 vs 15, stated explicitly

`RAW=60 … FILES=15` exposes a conflation in §2.2's earlier phrasing "58 in-scope hits across 17
files". The two figures rest on **different bases**:

- **17 files contain a category-C hit** (the 60 raw hits) — the figure `cat_c_scan4.py` printed as
  `NEW_FILES`, and the one §2.2.5 compares against the PRD's "22 files".
- **15 files contain an *in-scope* hit** (the 58). `let_type_disambiguation_tests.rs` and
  `m9_error_cases.rs` hold exactly one hit each, and both are §2.2.3's trait-requirement-`let`
  exclusions — so removing those 2 hits removes 2 whole files.

Both are correct on their own basis; only the sentence that paired "58" with "17 files" was wrong.
**§2.2.5's "short by 5 hits / 5 files" comparison stands** — it is 17-vs-22, an apples-to-apples
comparison of *files containing a hit*. Cite **58 hits / 15 files** for the in-scope set and
**60 hits / 17 files** for the raw set; never mix them.

### 12.3 The full 60-row hit list

Every row emitted by 12.1, in scan order, with §2.2.4's classification and (for c0) its
CONFIRMED/PRESUMED evidence marker. The 2 rows marked *(§2.2.3 excluded)* are the
trait-requirement `let`s that are **not** part of the 58. This table is the appendix form of the
same data §2.2.4 presents by row-group; where they disagree, they do not — both were generated
from the same run, and 12.4 reconciles the totals.

| # | `file:line` | Declaration (comment-stripped) | Class | Evidence | Disposition |
|---:|---|---|---|---|---|
| 1 | `crates/reify-compiler/tests/collection_sub_tests.rs:426` | `param grade : Length = 8.8` | **c1** | CONFIRMED | **BREAKS — migrate** (δ₁) |
| 2 | `crates/reify-compiler/tests/collection_sub_tests.rs:510` | `structure Bolt { param grade : Length = 8.8 }` | **c1** | CONFIRMED | **BREAKS — migrate** (δ₁) |
| 3 | `crates/reify-compiler/tests/harness_langcore/let_annotation_type_mismatch_tests.rs:176` | `let x : Length = 5` | **c2** | CONFIRMED | **DELIBERATE NEGATIVE TEST — invert** (δ₁) |
| 4 | `crates/reify-compiler/tests/harness_langcore/let_annotation_type_mismatch_tests.rs:177` | `let y : Length = 0.5` | **c2** | CONFIRMED | **DELIBERATE NEGATIVE TEST — invert** (δ₁) |
| 5 | `crates/reify-compiler/tests/harness_langcore/let_annotation_type_mismatch_tests.rs:178` | `let z : Length = -5.0` | **c2** | CONFIRMED | **DELIBERATE NEGATIVE TEST — invert** (δ₁) |
| 6 | `crates/reify-compiler/tests/harness_langcore/let_annotation_type_mismatch_tests.rs:583` | `let d : Length = 5` | **c2** | CONFIRMED | **DELIBERATE NEGATIVE TEST — invert** (δ₁) |
| 7 | `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:2025` | `param axis: Length = 0` | c0 | CONFIRMED | BREAKS — migrate, lower-urgency |
| 8 | `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:2190` | `param axis: Length = 0` | **c1** | CONFIRMED | **BREAKS — migrate** (δ₁) |
| 9 | `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:2251` | `param axis: Length = 0` | **c1** | CONFIRMED | **BREAKS — migrate** (δ₁) |
| 10 | `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:2359` | `param axis: Length = 0` | **c1** | CONFIRMED | **BREAKS — migrate** (δ₁) |
| 11 | `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:2471` | `param cond: Length = 0` | c0 | CONFIRMED | BREAKS — migrate, lower-urgency |
| 12 | `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:2540` | `param cond: Length = 0` | c0 | CONFIRMED | BREAKS — migrate, lower-urgency |
| 13 | `crates/reify-compiler/tests/harness_langcore/let_type_disambiguation_tests.rs:296` | `let x : Length = 5.0` | —  *(§2.2.3 excluded)* | n/a | not counted in the 58 — trait-requirement `let`, a different mechanism |
| 14 | `crates/reify-compiler/tests/harness_traits/trait_assoc_type_conformance_tests.rs:31` | `param w : Length = 1` | c0 | CONFIRMED | BREAKS — migrate, lower-urgency |
| 15 | `crates/reify-compiler/tests/harness_traits/trait_assoc_type_conformance_tests.rs:76` | `param w : Length = 1` | **?** | CONFIRMED | UNCLASSIFIED — resolve before landing |
| 16 | `crates/reify-compiler/tests/harness_traits/trait_assoc_type_conformance_tests.rs:110` | `param w : Length = 1` | **c1** | CONFIRMED | **BREAKS — migrate** (δ₁) |
| 17 | `crates/reify-compiler/tests/harness_traits/trait_assoc_type_conformance_tests.rs:166` | `param w : Length = 1` | **c1** | CONFIRMED | **BREAKS — migrate** (δ₁) |
| 18 | `crates/reify-compiler/tests/harness_traits/trait_assoc_type_conformance_tests.rs:224` | `param w : Length = 1` | **?** | CONFIRMED | UNCLASSIFIED — resolve before landing |
| 19 | `crates/reify-compiler/tests/m9_error_cases.rs:276` | `let score : Mass = 1.5` | —  *(§2.2.3 excluded)* | n/a | not counted in the 58 — trait-requirement `let`, a different mechanism |
| 20 | `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:175` | `param zero_int   : Length = 0` | **c2** | CONFIRMED | **DELIBERATE NEGATIVE TEST — invert** (δ₁) |
| 21 | `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:176` | `param one_int    : Length = 1` | **c2** | CONFIRMED | **DELIBERATE NEGATIVE TEST — invert** (δ₁) |
| 22 | `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:177` | `param half_real  : Length = 0.5` | **c2** | CONFIRMED | **DELIBERATE NEGATIVE TEST — invert** (δ₁) |
| 23 | `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:178` | `param large_real : Length = 70.0` | **c2** | CONFIRMED | **DELIBERATE NEGATIVE TEST — invert** (δ₁) |
| 24 | `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:206` | `param neg_real : Length = -5.0` | **c2** | CONFIRMED | **DELIBERATE NEGATIVE TEST — invert** (δ₁) |
| 25 | `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:207` | `param neg_int  : Length = -1` | **c2** | CONFIRMED | **DELIBERATE NEGATIVE TEST — invert** (δ₁) |
| 26 | `crates/reify-compiler/tests/prelude_context_tests.rs:217` | `param x : Length = 42` | c0 | CONFIRMED | BREAKS — migrate, lower-urgency |
| 27 | `crates/reify-compiler/tests/purpose_compile_tests.rs:1453` | `param material : Length = 1.0` | **c1** | CONFIRMED | **BREAKS — migrate** (δ₁) |
| 28 | `crates/reify-compiler/tests/purpose_compile_tests.rs:1454` | `param youngs_modulus : Length = 200.0` | **c1** | CONFIRMED | **BREAKS — migrate** (δ₁) |
| 29 | `crates/reify-compiler/tests/purpose_compile_tests.rs:1516` | `param material : Length = 1.0` | c0 | CONFIRMED | BREAKS — migrate, lower-urgency |
| 30 | `crates/reify-compiler/tests/purpose_compile_tests.rs:1517` | `param youngs_modulus : Length = 200.0` | c0 | CONFIRMED | BREAKS — migrate, lower-urgency |
| 31 | `crates/reify-compiler/tests/purpose_compile_tests.rs:1523` | `param x : Length = 1.0` | c0 | CONFIRMED | BREAKS — migrate, lower-urgency |
| 32 | `crates/reify-compiler/tests/purpose_compile_tests.rs:1567` | `param z : Length = 5.0` | c0 | **PRESUMED** | BREAKS — migrate, lower-urgency |
| 33 | `crates/reify-eval/tests/collection_sub_eval.rs:437` | `structure Bolt { param grade : Length = 8.8 }` | **c1** | CONFIRMED | **BREAKS — migrate** (δ₁) |
| 34 | `crates/reify-eval/tests/determinacy_predicates.rs:509` | `param a : Length = 10` | **c1** | CONFIRMED | **BREAKS — migrate** (δ₁) |
| 35 | `crates/reify-eval/tests/eval_param_overrides.rs:1639` | `let module_b = compile_source("structure S { param p: Money = 0 }");` | c0 | CONFIRMED | BREAKS — migrate, lower-urgency |
| 36 | `crates/reify-eval/tests/harness_fea_solver_e2e/stress_sweep_degenerate.rs:417` | `param x: Length = 5` | c0 | CONFIRMED | BREAKS — migrate, lower-urgency |
| 37 | `crates/reify-eval/tests/harness_fea_solver_e2e/stress_sweep_degenerate.rs:418` | `param y: Length = 10` | c0 | CONFIRMED | BREAKS — migrate, lower-urgency |
| 38 | `crates/reify-eval/tests/purpose_activation.rs:2673` | `param material : Length = 100.0` | **c1** | CONFIRMED | **BREAKS — migrate** (δ₁) |
| 39 | `crates/reify-eval/tests/purpose_activation.rs:2674` | `param youngs_modulus : Length = 200.0` | **c1** | CONFIRMED | **BREAKS — migrate** (δ₁) |
| 40 | `crates/reify-eval/tests/purpose_activation.rs:2774` | `param z : Length = 5.0` | **c1** | CONFIRMED | **BREAKS — migrate** (δ₁) |
| 41 | `crates/reify-eval/tests/purpose_activation.rs:2825` | `param z : Length = -5.0` | **c1** | CONFIRMED | **BREAKS — migrate** (δ₁) |
| 42 | `crates/reify-syntax/src/ts_parser.rs:6338` | `"structure S { port a : in T { /* comment */ param x: Length = 1 } }",` | c0 | CONFIRMED | **EXCLUDED — parse-only** (§2.2.4b) |
| 43 | `crates/reify-syntax/src/ts_parser.rs:6364` | `let source = "structure S { port a : in T { param x: Length = 1 }  sub b = T() }";` | c0 | CONFIRMED | **EXCLUDED — parse-only** (§2.2.4b) |
| 44 | `crates/reify-syntax/src/ts_parser.rs:6420` | `let source = "structure S { param x: Length = 1  port a : in T { param y: Length …` | c0 | CONFIRMED | **EXCLUDED — parse-only** (§2.2.4b) |
| 45 | `crates/reify-syntax/src/ts_parser.rs:6450` | `let source = "/* comment */\nstructure S { param x: Length = 1 }";` | c0 | CONFIRMED | **EXCLUDED — parse-only** (§2.2.4b) |
| 46 | `crates/reify-syntax/src/ts_parser.rs:6466` | `let src = "/// A bracket for mounting.\nstructure Bracket {\n  param w: Length = …` | c0 | CONFIRMED | **EXCLUDED — parse-only** (§2.2.4b) |
| 47 | `crates/reify-syntax/src/ts_parser.rs:6477` | `let src = "/// Line one.\n/// Line two.\nstructure S {\n  param x: Length = 1\n}";` | c0 | CONFIRMED | **EXCLUDED — parse-only** (§2.2.4b) |
| 48 | `crates/reify-syntax/src/ts_parser.rs:6488` | `let src = "structure S {\n  param x: Length = 1\n}";` | c0 | CONFIRMED | **EXCLUDED — parse-only** (§2.2.4b) |
| 49 | `crates/reify-syntax/src/ts_parser.rs:6499` | `let src = "// Just a comment\nstructure S {\n  param x: Length = 1\n}";` | c0 | CONFIRMED | **EXCLUDED — parse-only** (§2.2.4b) |
| 50 | `crates/reify-test-support/src/helpers.rs:1077` | `structure Alpha { param x: Length = 1 }` | c0 | **PRESUMED** | BREAKS — migrate, lower-urgency |
| 51 | `crates/reify-test-support/src/helpers.rs:1078` | `structure Beta { param y: Length = 2 }` | c0 | **PRESUMED** | BREAKS — migrate, lower-urgency |
| 52 | `crates/reify-test-support/src/helpers.rs:1227` | `let source = "structure S { param x: Length = 42 }";` | c0 | CONFIRMED | BREAKS — migrate, lower-urgency |
| 53 | `gui/src-tauri/src/tests/engine_tests.rs:3658` | `r#"structure Bolt { param mass: Length = 1 }` | c0 | **PRESUMED** | BREAKS — migrate, lower-urgency |
| 54 | `gui/src-tauri/src/tests/engine_tests.rs:3884` | `r#"structure Bolt { param mass: Length = 1 }` | c0 | **PRESUMED** | BREAKS — migrate, lower-urgency |
| 55 | `gui/src-tauri/src/tests/engine_tests.rs:4145` | `let source = "structure Foo { param x: Length = 1 }";` | c0 | **PRESUMED** | BREAKS — migrate, lower-urgency |
| 56 | `gui/src-tauri/src/tests/engine_tests.rs:4162` | `let source = "structure Foo { param x: Length = 1 }\n// outside any def";` | c0 | **PRESUMED** | BREAKS — migrate, lower-urgency |
| 57 | `gui/src-tauri/src/tests/engine_tests.rs:4197` | `let source = "structure Foo { param x: Length = 1 }";` | c0 | **PRESUMED** | BREAKS — migrate, lower-urgency |
| 58 | `gui/src-tauri/src/tests/engine_tests.rs:4842` | `let source1 = "structure A { param x: Length = 1 }";` | c0 | **PRESUMED** | BREAKS — migrate, lower-urgency |
| 59 | `gui/src-tauri/src/tests/engine_tests.rs:4858` | `let source2 = "structure A { param x: Length = 1 }\nstructure B { param y: Length…` | c0 | **PRESUMED** | BREAKS — migrate, lower-urgency |
| 60 | `gui/src-tauri/src/tests/engine_tests.rs:4858` | `let source2 = "structure A { param x: Length = 1 }\nstructure B { param y: Length…` | c0 | **PRESUMED** | BREAKS — migrate, lower-urgency |

### 12.4 Reconciliation

| Bucket | # | Source of truth |
|---|---:|---|
| c1 — BREAKS | 15 | §2.2.4 c1 table |
| c2 — DELIBERATE-INVERT | 10 | §2.2.4 c2 table |
| c0 — actionable (12 CONFIRMED + 11 PRESUMED) | 23 | §2.2.4 c0 table, minus the parse-only rows |
| c0 — EXCLUDED-parse-only (`ts_parser.rs`) | 8 | §2.2.4b |
| UNCLASSIFIED (`trait_assoc_type_conformance_tests.rs:76,224`) | 2 | §2.2.4 |
| **In-scope total** | **58** | **§2.2, §10.2, §10.3 — the normative figure** |
| §2.2.3 exclusions (trait-requirement `let`) | 2 | §2.2.3 — *not* part of the 58 |
| **Raw scan total** | **60** | §2.2.2 (3)'s transcript (`NEW_TOTAL_HITS=60`) |

15 + 10 + 23 + 8 + 2 = **58** ✓ · 58 + 2 = **60** ✓ · `param` 54 + `let` 4 = **58** ✓ (§2.2.5b;
the 4 `let`s are rows 3-6, all `let_annotation_type_mismatch_tests.rs` c2 pins).

### 12.5 What this appendix does and does not durably replace

It makes **§2.2's 58 re-derivable from this file alone**. It does **not** re-house the other
`/tmp/5756-scratch/**` evidence this note cites — §5's instrumented gate-5 sweep transcript, §§6-9's
probe `.ri` files, `step13-stripper-fix-transcript.txt`, `dropped-keys.txt`. Those substantiate
counts that each have their own inline reproduction command in their own section, so a consumer can
re-run the measurement even after the scratch directory is gone; the scratch files are corroborating
detail, not the sole executable form of a normative method. §2.2 was the one place where that was
not true, which is why it — and only it — is reproduced here in full.
