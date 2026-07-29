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

*(filled by step-1)*

## §2 Item 7a — §6.2 re-confirmation: gates 3+4, δ₁'s blast radius (step-2)

*(filled by step-2)*

## §3 Item 7b — §6.3 re-confirmation: gate 1, β/γ's blast radius (step-3)

*(filled by step-3)*

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
