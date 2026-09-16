# Struct-ctor field-type conformance — corpus survey

**Base commit:** `c528d054457a34a60c288cc4520bab0deb65b625`
**Tool:** `crates/reify-compiler/tests/harness_compilation_surface/ctor_conformance_corpus_survey.rs`
**Design:** `docs/prds/struct-ctor-field-type-conformance.md` (task β, §8)
**Sites:** 16 in the tracked `.ri` corpus; 272 in inline Rust fixtures
**Corpus:** 701 members of the `tracked .ri corpus` (enumeration parity floor 100); 3198 snippets extracted from the `inline Rust fixture hosts` (enumeration parity floor 300 hosts)
**`.ri` coverage:** 696 surveyed, 5 not surveyed, 78 partial

**Drifted corpus members since the anchor:** 2 tracked corpus members — `.ri` files, `.rs` hosts,
or both — differ between the anchor and the commit surveyed, so for those files
the anchor names OLDER bytes than the rows below describe. The list is filtered
to the two corpora, so it names exactly the files whose bytes a row could
describe and no unrelated churn. They are disclosed rather than refused because
they are COMMITTED: each is reachable from the surveyed commit, so a reader can
read back exactly what was swept. (Uncommitted bytes are reachable from no
commit, which is why a dirty tree is refused outright instead — see
`stamp_decision`.)

- `crates/reify-compiler/tests/harness_compilation_surface/ctor_conformance_corpus_survey.rs`
- `crates/reify-test-support/tests/rust_fixture_scan.rs`

This is a point-in-time **snapshot**, not a freshness-gated golden file. γ will
legitimately invalidate it — that is the point. Its job is to enumerate and size,
once, at the base commit stamped above.

## Provenance

Every row and every count here is **machine-generated — zero hand-derived
entries**. The corpus is `git ls-files -- '*.ri'`; each member is compiled with
the α(+ε) warn-stage compiler in-process (`parse_with_stdlib` →
`compile_with_stdlib`) and every diagnostic carrying one of the seven
ctor-conformance codes becomes one row. The `file:line` comes from the
diagnostic's own label span; `expected`/`found` come from the label message.
Nothing below was typed in by hand, and a column that could not be recovered
renders as `—` rather than as a guess.

Three things to know before reading a row:

- **`line` is the CTOR CALL-SITE line, not the offending argument's line.** α
anchors the label at the `Foo(...)` call's own span (PRD §10 Q1;
`compile_builder/entities_phase.rs`), so a multi-line ctor reports the line of
its opening `Foo(`. The offending argument is named in the `field` column and
sits within that call — e.g.
`examples/trajectory/printer_print_envelope.ri:169` is the `TOTSShaper(` line,
while `velocity_limit: 300.0` is three lines further down.
- **`def` is whatever identifier sits at that anchor, and `def source` says where
it came from.** The recovery reads an identifier followed by `(` — which cannot
by itself tell a `structure def` ctor from a plain function call. A few rows
carry codes that reach this survey from a NON-ctor path (selector composition,
overload resolution), where that identifier is a *function* name. Those are not
left in the actionable group: a recovered name is cross-checked against every
`structure def` declared in the corpus and the stdlib, and a name that does not
resolve is filed under *name recovered, but it is not a known structure def*.
- **A `—` in `def` is explained, not asserted.** The `def source` column carries
the machine-derived reason recovery failed for that specific row (span starts at
a non-identifier, identifier not followed by `(`, span out of range, …), so no
prose here has to guess a cause on a reader's behalf.

The **`disposition` column is γ's RULING**, projected from the site's measured
severity and the two per-site waiver tables (`CTOR_CONFORMANCE_CORPUS_RESIDUAL`
in the generator, `CTOR_CONFORMANCE_MIGRATION_DEBT` in the sibling
`examples_smoke.rs`) rather than typed here. It has four states, and they call
for four DIFFERENT actions:

- **`deferred`** names the LIVE task that owns retiring the site, and the reason
migrating it here would destroy something — most of these are committed RED
before-images whose violation IS the fixture's content. Leave them alone.
- **`n/a`** carries a ctor-conformance CODE but at **Error** severity, which is
outside the zero-ctor-conformance-warnings signal entirely. Every such row today
is a deliberate REJECTION fixture reached from a NON-ctor path (selector
composition, overload resolution, trait conformance): the rejection IS the
behaviour under test. **Not actionable, and not residual either** — it carries no
owner because it needs none, and reading it as unclaimed work would send you to
delete another PRD’s signal.
- **`unattributed`** is a warning claimed by nobody: that is the actionable
state, and after γ the tracked `.ri` corpus holds none.
- **`census`** is every row from the **inline** half — a Reify snippet embedded
in a Rust test fixture. Those sites are owned by **#5306**, which is chartered to
fix them; the task that enumerated them was chartered not to. They are listed so
the class is countable and cannot recur unnoticed on the next severity change,
and so #5306 inherits a list instead of a search. **Do not read a census row as
unclaimed work, and do not read it as waived either** — no waiver table names it,
because the tables key on `.ri` files.

The **`hint` column is ADVISORY**, derived purely from the (expected, found)
type pair. It is **not** a D9 ruling. PRD §4 D9 defines the split between class
(1) *call-site bug* and class (2) *wrong declared field type* as "per-case
judgment … whichever is the actual bug" and assigns it to **γ**. γ has now
ruled, and the ruling is the `disposition` column beside the hint: where the two
disagree, the disposition wins. What β decided mechanically is the `owner`
grouping below.

## Format (PRD §10 Q6)

**Q6 is answered here — grouped by D9 owner class, with a flat
`(file, line, field)`-sorted table inside each group.** Grouping first by owner
makes the FEA do-not-touch partition unmissable for γ, whose actual consumption
question is *which sites may I touch*; a flat table inside each group keeps the
result directly sizable and sortable. Recorded in this artifact header rather
than by editing the PRD's §10, which the sibling α/γ/δ/ζ tasks concurrently read.

## Sites

### FEA — deferred to v0.6 (DO NOT FIX HERE) — 4 site(s)

Per PRD §4 D9, these defs are declared in the FEA stdlib modules: γ may make
**call-site changes ONLY**. Field-type flips remain v0.6-owned
(`docs/prds/v0_6/fea-load-support-selector-migration.md`). **DO NOT FIX the
declared field types here.**

| site | def | def source | field | expected | found | code | severity | hint (advisory) | disposition (γ ruling) | message |
|---|---|---|---|---|---|---|---|---|---|---|
| `tests/prd-gate/fixtures/dcr_load_ctor_dimension_silent.ri:28` | PointLoad | ctor call-site anchor | force | Real | Scalar[m·kg·s^-2] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | deferred — owned by #6941: leaf γ2: PointLoad.force is declared `Real` in fea_multi_case.ri, so the units-CORRECT `force: 5000N` warns; γ2 retypes the FIELD, and the call site is already right | argument 'force' has type 'Scalar[m·kg·s^-2]' but param 'force' requires type 'Real' |
| `tests/prd-gate/fixtures/dcr_load_ctor_dimension_silent.ri:31` | TractionLoad | ctor call-site anchor | traction | Real | Scalar[kg·m^-1·s^-2] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | deferred — owned by #6941: leaf γ2: TractionLoad.traction is declared `Real` in fea_multi_case.ri; same retype, and TractionLoad reaches no solver at all today (INV-SF-3) | argument 'traction' has type 'Scalar[kg·m^-1·s^-2]' but param 'traction' requires type 'Real' |
| `tests/prd-gate/fixtures/dcr_solver_load_dropped_dimensioned.ri:34` | PointLoad | ctor call-site anchor | force | Real | Scalar[m·kg·s^-2] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | deferred — owned by #6922: leaf γ1 headline inversion: the units-CORRECT `force: 1000N` contributes EXACTLY ZERO force to solve_elastic_static (max_von_mises 0, iterations 0) where the bare control contributes 1000 N | argument 'force' has type 'Scalar[m·kg·s^-2]' but param 'force' requires type 'Real' |
| `tests/prd-gate/fixtures/dcr_yield_stress_dimension_silent.ri:31` | Steel_AISI_1045 | ctor call-site anchor | yield_stress | Scalar[kg·m^-1·s^-2] | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | deferred — owned by #6941: leaf β, boundary row B5: `yield_stress: 310mm` is stored as Some(0.31 m) and material_field_si reads it as 0.31 Pa, at exit 0 with zero diagnostics | argument 'yield_stress' has type 'Scalar[m]' but param 'yield_stress' requires type 'Scalar[kg·m^-1·s^-2]'; pass a dimensioned Pressure literal such as `1kg/m/s^2` |

### non-FEA structure def — γ per-case judgment — 8 site(s)

The recovered name IS a `structure def` declared in the corpus or the stdlib,
and it is not FEA-owned. D9's per-case judgment applied here, and **γ has
now ruled every Warning row in this group** — read the `disposition` column.
A site γ judged a call-site bug was fixed and is simply absent below; a site γ
deferred names the LIVE task that owns retiring it.

That check is against ONE GLOBAL namespace — *some* corpus or stdlib file
declares the name, not necessarily one this row's file can see. See named
limitation 3 below before treating a row here as actionable.

| site | def | def source | field | expected | found | code | severity | hint (advisory) | disposition (γ ruling) | message |
|---|---|---|---|---|---|---|---|---|---|---|
| `examples/trajectory/printer_print_envelope.ri:179` | TOTSShaper | ctor call-site anchor | acceleration_limit | Scalar[m·s^-2] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | deferred — owned by #5847: un-migrated examples/ call site that cannot be dimensioned in isolation; waived per-site in CTOR_CONFORMANCE_MIGRATION_DEBT and retired by its owning task's own diff | argument 'acceleration_limit' has type 'Real' but param 'acceleration_limit' requires type 'Scalar[m·s^-2]'; pass a dimensioned Acceleration literal such as `1m/s^2` |
| `examples/trajectory/printer_print_envelope.ri:179` | TOTSShaper | ctor call-site anchor | velocity_limit | Scalar[m·s^-1] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | deferred — owned by #5847: un-migrated examples/ call site that cannot be dimensioned in isolation; waived per-site in CTOR_CONFORMANCE_MIGRATION_DEBT and retired by its owning task's own diff | argument 'velocity_limit' has type 'Real' but param 'velocity_limit' requires type 'Scalar[m·s^-1]'; pass a dimensioned Velocity literal such as `1m/s` |
| `tests/prd-gate/fixtures/dcr_material_dimension_silent.ri:24` | Material | ctor call-site anchor | youngs_modulus | Scalar[kg·m^-1·s^-2] | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | deferred — owned by #6941: leaf β, boundary row B4: `youngs_modulus: 200mm` is read as 0.2 Pa by material_field_si, measured 1e12x wrong at exit 0 with zero Error diagnostics | argument 'youngs_modulus' has type 'Scalar[m]' but param 'youngs_modulus' requires type 'Scalar[kg·m^-1·s^-2]'; pass a dimensioned Pressure literal such as `1kg/m/s^2` |
| `tests/prd-gate/fixtures/dcr_reader_ctor_dimension_silent.ri:28` | MassProperties | ctor call-site anchor | mass | Scalar[kg] | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | deferred — owned by #6922: leaf ε: `MassProperties(mass: 2m)` is read as 2.0 kg by the blind cell_f64 copy, while the dimension-checking cell_mass_f64 sits unused ~300 lines away | argument 'mass' has type 'Scalar[m]' but param 'mass' requires type 'Scalar[kg]'; pass a dimensioned Mass literal such as `1kg` |
| `tests/prd-gate/fixtures/dcr_reader_ctor_dimension_silent.ri:29` | ZVShaper | ctor call-site anchor | target_frequency | Scalar[s^-1] | Scalar[rad·s^-1] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | deferred — owned by #6941: leaf ζ: `ZVShaper(target_frequency: 50rad/s)` is stored verbatim as rad·s^-1 and the Hz->rad/s marshalling then multiplies by 2π — a 6.28x error | argument 'target_frequency' has type 'Scalar[rad·s^-1]' but param 'target_frequency' requires type 'Scalar[s^-1]'; pass a dimensioned Frequency literal |
| `tests/prd-gate/fixtures/dcr_reader_ctor_dimension_silent.ri:31` | AsPrintedOptions | ctor call-site anchor | line_width | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | deferred — owned by #6922: leaf η: `AsPrintedOptions(line_width: 0.4)` is read as 0.4 METRES by field_scalar — a 1000x error on a 0.4mm extrusion | argument 'line_width' has type 'Real' but param 'line_width' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `tests/prd-gate/fixtures/dcr_reader_ctor_dimension_silent.ri:32` | FDMCouponOverride | ctor call-site anchor | ex | Scalar[kg·m^-1·s^-2] | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | deferred — owned by #6922: leaf η: `FDMCouponOverride(ex: 2mm)` stores 0.002 m and the dimension-blind opt_f64 reads it as 0.002 Pa | argument 'ex' has type 'Scalar[m]' but param 'ex' requires type 'Scalar[kg·m^-1·s^-2]'; pass a dimensioned Pressure literal such as `1kg/m/s^2` |
| `tests/prd-gate/fixtures/dcr_shaper_frequency_dimension_silent.ri:37` | ZVShaper | ctor call-site anchor | target_frequency | Scalar[s^-1] | Scalar[rad·s^-1] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | deferred — owned by #6941: leaf ζ signal fixture: the same 6.28x error, but CONSUMED via input_shape so read_scalar_si actually runs — the ctor alone never reaches the reader | argument 'target_frequency' has type 'Scalar[rad·s^-1]' but param 'target_frequency' requires type 'Scalar[s^-1]'; pass a dimensioned Frequency literal |

### name recovered, but it is not a known structure def — needs manual triage — 2 site(s)

An identifier was recovered at the diagnostic's anchor, but it is not a
`structure def` declared anywhere in the corpus or the stdlib. Recovery reads
an identifier followed by `(`, which cannot distinguish a ctor from a plain
function call, so these are typically FUNCTION names reaching the survey from
a non-ctor path (selector composition, overload resolution) — the `severity`
and `message` columns show which. Held out of the actionable group rather than
sized into it. **Triage manually before touching.**

| site | def | def source | field | expected | found | code | severity | hint (advisory) | disposition (γ ruling) | message |
|---|---|---|---|---|---|---|---|---|---|---|
| `crates/reify-eval/tests/fixtures/selectors/bt1_wrong_kind_union.ri:15` | union | ctor call-site anchor | — | — | — | `SelectorKindMismatch` | Error | no mechanical hint — γ per-case judgment | n/a — Error severity, outside the ctor-conformance warning signal: nothing to retire | selector composition kind mismatch: cannot compose FaceSelector and EdgeSelector |
| `crates/reify-eval/tests/fixtures/selectors/bt6_kind_typed_param.ri:24` | needs_face | ctor call-site anchor | — | — | — | `SelectorKindMismatch` | Error | no mechanical hint — γ per-case judgment | n/a — Error severity, outside the ctor-conformance warning signal: nothing to retire | no matching overload for needs_face(EdgeSelector), candidates: needs_face(FaceSelector) -> Int |

### unattributed def — needs manual triage — 2 site(s)

No def name could be attributed. The `def source` column gives the
machine-derived reason PER ROW rather than asserting one cause for the group:
known shapes that land here include the sub `=` per-arg anchor and param
default-initializer checks, both of which anchor the label somewhere other
than a `Def(` call site. Deliberately its own group: folding an
unattributable site into the touchable pile is the one classification error
with a real cost. **Triage manually before touching.**

| site | def | def source | field | expected | found | code | severity | hint (advisory) | disposition (γ ruling) | message |
|---|---|---|---|---|---|---|---|---|---|---|
| `tests/prd-gate/fixtures/curvature_rad_literal.ri:12` | — | unrecovered: identifier not followed by `(` | kc | Scalar[m^-1] | Scalar[rad·m^-1] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | deferred — owned by #6179: angle-completion leaf α, boundary row B1: CURVATURE is m^-1 pre-α, so the rad·m^-1 initializer mismatches; the fixture's own header calls that check-time flip α's signal | argument 'kc' has type 'Scalar[rad·m^-1]' but param 'kc' requires type 'Scalar[m^-1]'; pass a dimensioned AbsorptionCoeff literal |
| `tests/prd-gate/fixtures/raw_lambda_material_field_rejected.ri:21` | — | unrecovered: identifier not followed by `(` | material | — | — | `TypeNotConformingToTrait` | Error | no mechanical hint — γ per-case judgment | n/a — Error severity, outside the ctor-conformance warning signal: nothing to retire | type 'Field<<error>, AnisotropicMaterial>' does not conform to trait 'ConstitutiveLaw' required by param 'material' |

## Inline Rust fixtures

Reify snippets embedded in Rust test sources as raw-string literals, swept by
the SAME pipeline as the tracked `.ri` corpus above. A row's `site` cell is the
HOST `.rs` position to open; the `snippet line` cell locates the declaration
inside the literal.

Every row here carries the `census` disposition: these sites are owned by
**#5306**, and are enumerated rather than fixed so the class is countable and
cannot recur unnoticed on the next severity change.

### FEA — deferred to v0.6 (DO NOT FIX HERE) — 9 site(s)

| site | snippet line | def | def source | field | expected | found | code | severity | hint (advisory) | disposition (γ ruling) | message |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:475` | 5 | PressureLoad | ctor call-site anchor | face | FaceSelector | Frame3 | `ArgTypeMismatch` | Warning | selector field given a coordinate pose — a pose locates a datum, it does not name a region target | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'face' has type 'Frame3' but param 'face' requires selector type 'FaceSelector'; a coordinate pose is not a region target; select a face/edge/vertex instead |
| `crates/reify-compiler/tests/multi_load_case_stdlib_tests.rs:309` | 2 | LoadCase | ctor call-site anchor | loads | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'Int' does not conform to trait 'Load' required by param 'loads' |
| `crates/reify-compiler/tests/multi_load_case_stdlib_tests.rs:309` | 2 | LoadCase | ctor call-site anchor | loads | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'Int' does not conform to trait 'Load' required by param 'loads' |
| `crates/reify-compiler/tests/multi_load_case_stdlib_tests.rs:309` | 2 | LoadCase | ctor call-site anchor | loads | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'Int' does not conform to trait 'Load' required by param 'loads' |
| `crates/reify-compiler/tests/multi_load_case_stdlib_tests.rs:344` | 2 | LoadCase | ctor call-site anchor | supports | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'Int' does not conform to trait 'Support' required by param 'supports' |
| `crates/reify-compiler/tests/multi_load_case_stdlib_tests.rs:344` | 2 | LoadCase | ctor call-site anchor | supports | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'Int' does not conform to trait 'Support' required by param 'supports' |
| `crates/reify-compiler/tests/multi_load_case_stdlib_tests.rs:344` | 2 | LoadCase | ctor call-site anchor | supports | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'Int' does not conform to trait 'Support' required by param 'supports' |
| `crates/reify-compiler/tests/multi_load_case_stdlib_tests.rs:380` | 2 | LoadCase | ctor call-site anchor | loads | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'FixedSupport' does not conform to trait 'Load' required by param 'loads' |
| `crates/reify-eval/tests/structure_instance_e2e.rs:311` | 6 | PointLoad | ctor call-site anchor | mat | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'PointLoad' does not conform to trait 'ElasticMaterial' required by param 'mat' |

### non-FEA structure def — γ per-case judgment — 120 site(s)

| site | snippet line | def | def source | field | expected | found | code | severity | hint (advisory) | disposition (γ ruling) | message |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `crates/reify-compiler/tests/harness_mechanics/modal_mechanism_compile.rs:266` | 6 | RayleighDamping | ctor call-site anchor | alpha | Scalar[s^-1] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'alpha' has type 'Real' but param 'alpha' requires type 'Scalar[s^-1]'; pass a dimensioned Frequency literal |
| `crates/reify-compiler/tests/harness_mechanics/modal_mechanism_compile.rs:266` | 6 | RayleighDamping | ctor call-site anchor | beta | Scalar[s] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'beta' has type 'Real' but param 'beta' requires type 'Scalar[s]'; pass a dimensioned Time literal such as `1s` |
| `crates/reify-compiler/tests/harness_mechanics/modal_options_validation_tests.rs:556` | 2 | RayleighDamping | ctor call-site anchor | alpha | Scalar[s^-1] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'alpha' has type 'Real' but param 'alpha' requires type 'Scalar[s^-1]'; pass a dimensioned Frequency literal |
| `crates/reify-compiler/tests/harness_mechanics/modal_options_validation_tests.rs:556` | 2 | RayleighDamping | ctor call-site anchor | beta | Scalar[s] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'beta' has type 'Real' but param 'beta' requires type 'Scalar[s]'; pass a dimensioned Time literal such as `1s` |
| `crates/reify-compiler/tests/harness_mechanics/modal_options_validation_tests.rs:632` | 2 | RayleighDamping | ctor call-site anchor | alpha | Scalar[s^-1] | Scalar[s] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'alpha' has type 'Scalar[s]' but param 'alpha' requires type 'Scalar[s^-1]'; pass a dimensioned Frequency literal |
| `crates/reify-compiler/tests/harness_mechanics/modal_options_validation_tests.rs:632` | 2 | RayleighDamping | ctor call-site anchor | beta | Scalar[s] | Scalar[s^-1] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'beta' has type 'Scalar[s^-1]' but param 'beta' requires type 'Scalar[s]'; pass a dimensioned Time literal such as `1s` |
| `crates/reify-compiler/tests/harness_mechanics/modal_options_validation_tests.rs:2328` | 12 | ForcingTimeHistory | ctor call-site anchor | part | Part | String | `TypeNotConformingToStructureRef` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'part' has type 'String' but param 'part' requires structure type 'Part' |
| `crates/reify-compiler/tests/harness_mechanics/modal_options_validation_tests.rs:2521` | 2 | StepForce | ctor call-site anchor | at | Selector | Real | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'at' has type 'Real' but param 'at' requires selector type 'Selector' |
| `crates/reify-compiler/tests/harness_mechanics/modal_options_validation_tests.rs:2562` | 2 | StepForce | ctor call-site anchor | at | Selector | String | `ArgTypeMismatch` | Warning | selector field given a string — typed ctor such as face(b, "x_max") or vertex(b, "tip") is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'at' has type 'String' but param 'at' requires selector type 'Selector' |
| `crates/reify-compiler/tests/harness_mechanics/modal_options_validation_tests.rs:2603` | 2 | StepForce | ctor call-site anchor | at | Selector | Int | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'at' has type 'Int' but param 'at' requires selector type 'Selector' |
| `crates/reify-compiler/tests/harness_mechanics/modal_options_validation_tests.rs:2645` | 3 | StepForce | ctor call-site anchor | at | Selector | Real | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'at' has type 'Real' but param 'at' requires selector type 'Selector' |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:172` | 4 | Plain | ctor call-site anchor | m | Material | Plain | `TypeNotConformingToStructureRef` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'm' has type 'Plain' but param 'm' requires structure type 'Material' |
| `crates/reify-compiler/tests/harness_physical_modeling/vec3_type_tests.rs:360` | 5 | AxisHolder | ctor call-site anchor | axis | Vector3<Scalar[m]> | Real | `TypeNotConformingToVector` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'axis' has type 'Real' but param 'axis' requires vector type 'Vector3<Scalar[m]>' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:98` | 4 | Widget | ctor call-site anchor | label | String | Int | `ArgTypeMismatch` | Warning | string field given a non-string literal | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Int' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:312` | 5 | Holder | ctor call-site anchor | mat | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'NotAMaterial' does not conform to trait 'MaterialSpec' required by param 'mat' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:362` | 5 | Holder | ctor call-site anchor | loc | FaceSelector | EdgeSelector | `SelectorKindMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'loc' has selector kind 'EdgeSelector' but param 'loc' requires selector kind 'FaceSelector' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:410` | 4 | Holder | ctor call-site anchor | loc | FaceSelector | String | `ArgTypeMismatch` | Warning | selector field given a string — typed ctor such as face(b, "x_max") or vertex(b, "tip") is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'loc' has type 'String' but param 'loc' requires selector type 'FaceSelector' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:513` | 5 | Holder | ctor call-site anchor | loc | FaceSelector | Frame3 | `ArgTypeMismatch` | Warning | selector field given a coordinate pose — a pose locates a datum, it does not name a region target | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'loc' has type 'Frame3' but param 'loc' requires selector type 'FaceSelector'; a coordinate pose is not a region target; select a face/edge/vertex instead |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:548` | 5 | Holder | ctor call-site anchor | loc | FaceSelector | Transform3 | `ArgTypeMismatch` | Warning | selector field given a coordinate pose — a pose locates a datum, it does not name a region target | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'loc' has type 'Transform3' but param 'loc' requires selector type 'FaceSelector'; a coordinate pose is not a region target; select a face/edge/vertex instead |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:698` | 4 | Widget | ctor call-site anchor | label | String | Int | `ArgTypeMismatch` | Warning | string field given a non-string literal | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Int' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:715` | 4 | Widget | ctor call-site anchor | label | String | Int | `ArgTypeMismatch` | Warning | string field given a non-string literal | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Int' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:715` | 4 | Widget | ctor call-site anchor | label | String | Int | `ArgTypeMismatch` | Warning | string field given a non-string literal | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Int' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:725` | 4 | Widget | ctor call-site anchor | label | String | Int | `ArgTypeMismatch` | Warning | string field given a non-string literal | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Int' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:725` | 4 | Widget | ctor call-site anchor | label | String | Int | `ArgTypeMismatch` | Warning | string field given a non-string literal | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Int' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:734` | 6 | Widget | ctor call-site anchor | label | String | Int | `ArgTypeMismatch` | Warning | string field given a non-string literal | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Int' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:743` | 5 | Widget | ctor call-site anchor | label | String | Int | `ArgTypeMismatch` | Warning | string field given a non-string literal | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Int' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:751` | 4 | Widget | ctor call-site anchor | label | String | Int | `ArgTypeMismatch` | Warning | string field given a non-string literal | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Int' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:751` | 4 | Widget | ctor call-site anchor | label | String | Int | `ArgTypeMismatch` | Warning | string field given a non-string literal | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Int' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:751` | 4 | Widget | ctor call-site anchor | label | String | Int | `ArgTypeMismatch` | Warning | string field given a non-string literal | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Int' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:751` | 4 | Widget | ctor call-site anchor | label | String | Int | `ArgTypeMismatch` | Warning | string field given a non-string literal | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Int' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:751` | 4 | Widget | ctor call-site anchor | label | String | Int | `ArgTypeMismatch` | Warning | string field given a non-string literal | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Int' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:751` | 4 | Widget | ctor call-site anchor | label | String | Int | `ArgTypeMismatch` | Warning | string field given a non-string literal | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Int' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:757` | 3 | Widget | ctor call-site anchor | label | String | Int | `ArgTypeMismatch` | Warning | string field given a non-string literal | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Int' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:830` | 3 | Widget | ctor call-site anchor | label | String | Int | `ArgTypeMismatch` | Warning | string field given a non-string literal | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Int' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:1208` | 4 | Anchor | ctor call-site anchor | origin | Point3<Scalar[m]> | String | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'origin' has type 'String' but param 'origin' requires type 'Point3<Scalar[m]>' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:1250` | 4 | Anchor | ctor call-site anchor | origin | Point3<Scalar[m]> | String | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'origin' has type 'String' but param 'origin' requires type 'Point3<Scalar[m]>' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:1304` | 4 | Body | ctor call-site anchor | inertia | Matrix3x3<Scalar[m^2·kg]> | String | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'inertia' has type 'String' but param 'inertia' requires type 'Matrix3x3<Scalar[m^2·kg]>' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:1321` | 4 | Body | ctor call-site anchor | inertia | Matrix3x3<Scalar[m^2·kg]> | List<String> | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'inertia' has type 'List<String>' but param 'inertia' requires type 'Matrix3x3<Scalar[m^2·kg]>' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:1350` | 4 | Body | ctor call-site anchor | inertias | Matrix3x3<Scalar[m^2·kg]> | String | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'inertias' has type 'String' but param 'inertias' requires type 'Matrix3x3<Scalar[m^2·kg]>' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:1372` | 4 | Body | ctor call-site anchor | stress | Tensor2x3<Scalar[kg·m^-1·s^-2]> | String | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'stress' has type 'String' but param 'stress' requires type 'Tensor2x3<Scalar[kg·m^-1·s^-2]>' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:1466` | 4 | Holder | ctor call-site anchor | mode_shape | Field<Point3<Scalar[m]>, Vector3<Scalar[m]>> | String | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'mode_shape' has type 'String' but param 'mode_shape' requires type 'Field<Point3<Scalar[m]>, Vector3<Scalar[m]>>' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:1487` | 4 | Holder | ctor call-site anchor | modes | Field<Point3<Scalar[m]>, Vector3<Scalar[m]>> | String | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'modes' has type 'String' but param 'modes' requires type 'Field<Point3<Scalar[m]>, Vector3<Scalar[m]>>' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:1770` | 3 | W | ctor call-site anchor | label | String | Int | `ArgTypeMismatch` | Warning | string field given a non-string literal | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Int' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:1775` | 3 | W | ctor call-site anchor | flag | Bool | String | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'flag' has type 'String' but param 'flag' requires type 'Bool' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:1780` | 3 | W | ctor call-site anchor | n | Int | String | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'n' has type 'String' but param 'n' requires type 'Int' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:1785` | 3 | W | ctor call-site anchor | mag | Real | String | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'mag' has type 'String' but param 'mag' requires type 'Real' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:1862` | 3 | W | ctor call-site anchor | p | Scalar[kg·m^-1·s^-2] | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'p' has type 'Scalar[m]' but param 'p' requires type 'Scalar[kg·m^-1·s^-2]'; pass a dimensioned Pressure literal such as `1kg/m/s^2` |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:1867` | 3 | W | ctor call-site anchor | p | Scalar[m·s^-1] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'p' has type 'Real' but param 'p' requires type 'Scalar[m·s^-1]'; pass a dimensioned Velocity literal such as `1m/s` |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:1872` | 3 | W | ctor call-site anchor | p | Scalar[m·s^-1] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'p' has type 'Int' but param 'p' requires type 'Scalar[m·s^-1]'; pass a dimensioned Velocity literal such as `1m/s` |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:1877` | 3 | W | ctor call-site anchor | d | Scalar[kg·m^-3] | String | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'd' has type 'String' but param 'd' requires type 'Scalar[kg·m^-3]'; pass a dimensioned Density literal such as `1kg/m^3` |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:1882` | 3 | W | ctor call-site anchor | d | Scalar[kg·m^-3] | Bool | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'd' has type 'Bool' but param 'd' requires type 'Scalar[kg·m^-3]'; pass a dimensioned Density literal such as `1kg/m^3` |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:1944` | 7 | Steel | ctor call-site anchor | density | Scalar[kg·m^-3] | String | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'density' has type 'String' but param 'density' requires type 'Scalar[kg·m^-3]'; pass a dimensioned Density literal such as `1kg/m^3` |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:1944` | 7 | Steel | ctor call-site anchor | youngs_modulus | Scalar[kg·m^-1·s^-2] | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'youngs_modulus' has type 'Scalar[m]' but param 'youngs_modulus' requires type 'Scalar[kg·m^-1·s^-2]'; pass a dimensioned Pressure literal such as `1kg/m/s^2` |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:2378` | 3 | W | ctor call-site anchor | label | String | Scalar<Q> | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Scalar<Q>' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:2383` | 3 | W | ctor call-site anchor | flag | Bool | Scalar<Q> | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'flag' has type 'Scalar<Q>' but param 'flag' requires type 'Bool' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:2388` | 3 | W | ctor call-site anchor | len | Scalar[m] | Scalar[kg] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'len' has type 'Scalar[kg]' but param 'len' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:2393` | 3 | W | ctor call-site anchor | len | Scalar[m] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'len' has type 'Int' but param 'len' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:2511` | 4 | Joint | ctor call-site anchor | axis | Scalar[m] | Scalar[kg] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'axis' has quantity 'Scalar[kg]' but param 'axis' requires quantity 'Scalar[m]' (the compared shape 'Vector3<Scalar[kg]>' is otherwise accepted at 'Vector3<Scalar[m]>'; only the quantity slot disagrees) |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:2606` | 4 | Frame | ctor call-site anchor | dir | Real | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'dir' has quantity 'Scalar[m]' but param 'dir' requires quantity 'Real' (the compared shape 'Vector3<Scalar[m]>' is otherwise accepted at 'Vector3<Real>'; only the quantity slot disagrees) |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:2711` | 4 | Joint | ctor call-site anchor | axis | Vector3<Scalar[m]> | Vector2<Scalar[m]> | `TypeNotConformingToVector` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'axis' has type 'Vector2<Scalar[m]>' but param 'axis' requires vector type 'Vector3<Scalar[m]>' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:2774` | 4 | Origin | ctor call-site anchor | origin | Real | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'origin' has quantity 'Scalar[m]' but param 'origin' requires quantity 'Real' (the compared shape 'Point3<Scalar[m]>' is otherwise accepted at 'Point3<Real>'; only the quantity slot disagrees) |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:2781` | 4 | Origin | ctor call-site anchor | origin | Real | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'origin' has quantity 'Scalar[m]' but param 'origin' requires quantity 'Real' (the compared shape 'Point3<Scalar[m]>' is otherwise accepted at 'Point3<Real>'; only the quantity slot disagrees) |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:2861` | 4 | Bead | ctor call-site anchor | centerline | Real | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'centerline' has quantity 'Scalar[m]' but param 'centerline' requires quantity 'Real' (the compared shape 'Point3<Scalar[m]>' is otherwise accepted at 'Point3<Real>'; only the quantity slot disagrees) |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:2925` | 4 | Anchor | ctor call-site anchor | origin | Scalar[m] | Scalar[kg] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'origin' has quantity 'Scalar[kg]' but param 'origin' requires quantity 'Scalar[m]' (the compared shape 'Point3<Scalar[kg]>' is otherwise accepted at 'Point3<Scalar[m]>'; only the quantity slot disagrees) |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:2984` | 5 | Anchor | ctor call-site anchor | origin | Scalar[m] | Scalar[kg] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'origin' has quantity 'Scalar[kg]' but param 'origin' requires quantity 'Scalar[m]' (the compared shape 'Point3<Scalar[kg]>' is otherwise accepted at 'Point3<Scalar[m]>'; only the quantity slot disagrees) |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:3094` | 4 | Anchor | ctor call-site anchor | origin | Point3<Scalar[m]> | Point2<Scalar[m]> | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'origin' has type 'Point2<Scalar[m]>' but param 'origin' requires type 'Point3<Scalar[m]>' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:3162` | 4 | Joint | ctor call-site anchor | axis | Vector3<Scalar[m]> | String | `TypeNotConformingToVector` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'axis' has type 'String' but param 'axis' requires vector type 'Vector3<Scalar[m]>' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:3201` | 4 | Body | ctor call-site anchor | inertia | Scalar[m^2·kg] | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'inertia' has quantity 'Scalar[m]' but param 'inertia' requires quantity 'Scalar[m^2·kg]' (the compared shape 'Tensor2x3<Scalar[m]>' is otherwise accepted at 'Matrix3x3<Scalar[m^2·kg]>'; only the quantity slot disagrees) |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:3248` | 4 | Jacobian | ctor call-site anchor | jac | Real | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'jac' has quantity 'Scalar[m]' but param 'jac' requires quantity 'Real' (the compared shape 'Tensor2x3<Scalar[m]>' is otherwise accepted at 'Matrix3x3<Real>'; only the quantity slot disagrees) |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:3360` | 4 | Widget11 | diagnostic prose | labl | — | — | `CtorUnknownField` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_UNKNOWN_FIELD: unknown named argument 'labl' in call to 'Widget11'; 'Widget11' has no parameter with that name |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:3450` | 4 | Widget11 | diagnostic prose | labl | — | — | `CtorUnknownField` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_UNKNOWN_FIELD: unknown named argument 'labl' in call to 'Widget11'; 'Widget11' has no parameter with that name |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:3450` | 4 | Widget11 | diagnostic prose | lable2 | — | — | `CtorUnknownField` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_UNKNOWN_FIELD: unknown named argument 'lable2' in call to 'Widget11'; 'Widget11' has no parameter with that name |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:3615` | 4 | Widget12 | diagnostic prose | — | — | — | `CtorArity` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_ARITY: Widget12() expects at most 1 argument, got 2 |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:3708` | 4 | Widget12 | diagnostic prose | — | — | — | `CtorArity` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_ARITY: Widget12() expects at most 1 argument, got 3 |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:3744` | 4 | W0 | diagnostic prose | — | — | — | `CtorArity` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_ARITY: W0() expects at most 0 arguments, got 1 |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:3773` | 4 | Widget12 | diagnostic prose | — | — | — | `CtorArity` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_ARITY: Widget12() expects at most 1 argument, got 2 |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:3889` | 3 | Widget11 | diagnostic prose | labl | — | — | `CtorUnknownField` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_UNKNOWN_FIELD: unknown named argument 'labl' in call to 'Widget11'; 'Widget11' has no parameter with that name |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:3913` | 5 | Widget11 | diagnostic prose | labl | — | — | `CtorUnknownField` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_UNKNOWN_FIELD: unknown named argument 'labl' in call to 'Widget11'; 'Widget11' has no parameter with that name |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:3942` | 3 | Widget12 | diagnostic prose | — | — | — | `CtorArity` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_ARITY: Widget12() expects at most 1 argument, got 2 |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:3962` | 4 | Widget13 | diagnostic prose | — | — | — | `CtorArity` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_ARITY: Widget13() expects at most 1 argument, got 3 |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:3962` | 4 | Widget13 | ctor call-site anchor | label | String | Int | `ArgTypeMismatch` | Warning | string field given a non-string literal | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Int' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:3962` | 4 | Widget13 | diagnostic prose | labl | — | — | `CtorUnknownField` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_UNKNOWN_FIELD: unknown named argument 'labl' in call to 'Widget13'; 'Widget13' has no parameter with that name |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:4036` | 4 | Widget14 | diagnostic prose | labl | — | — | `CtorUnknownField` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_UNKNOWN_FIELD: unknown named argument 'labl' in call to 'Widget14'; 'Widget14' has no parameter with that name |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:4078` | 4 | Widget15 | diagnostic prose | — | — | — | `CtorArity` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_ARITY: Widget15() expects at most 1 argument, got 3 |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:4078` | 4 | Widget15 | diagnostic prose | labl | — | — | `CtorUnknownField` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_UNKNOWN_FIELD: unknown named argument 'labl' in call to 'Widget15'; 'Widget15' has no parameter with that name |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:4126` | 4 | Widget11 | diagnostic prose | labl | — | — | `CtorUnknownField` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_UNKNOWN_FIELD: unknown named argument 'labl' in call to 'Widget11'; 'Widget11' has no parameter with that name |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:4294` | 7 | WidgetAutoTypo | diagnostic prose | zz | — | — | `CtorUnknownField` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_UNKNOWN_FIELD: unknown named argument 'zz' in call to 'WidgetAutoTypo'; 'WidgetAutoTypo' has no parameter with that name |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:4434` | 7 | WidgetAutoSurplus | diagnostic prose | — | — | — | `CtorArity` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_ARITY: WidgetAutoSurplus() expects at most 2 arguments, got 3 |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:4548` | 6 | WLet | diagnostic prose | — | — | — | `CtorArity` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_ARITY: WLet() expects at most 0 arguments, got 1 |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:4583` | 7 | WLetParam | diagnostic prose | — | — | — | `CtorArity` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_ARITY: WLetParam() expects at most 1 argument, got 3 |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:4618` | 8 | WMixed | diagnostic prose | — | — | — | `CtorArity` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_ARITY: WMixed() expects at most 2 arguments, got 3 |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:4680` | 6 | WPrivAuto | diagnostic prose | — | — | — | `CtorArity` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_ARITY: WPrivAuto() expects at most 0 arguments, got 1 |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:4761` | 7 | WLetMember | diagnostic prose | k | — | — | `CtorUnknownField` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_UNKNOWN_FIELD: unknown named argument 'k' in call to 'WLetMember'; 'WLetMember' has no parameter with that name |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:4771` | 7 | WAuxLet | diagnostic prose | k | — | — | `CtorUnknownField` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_UNKNOWN_FIELD: unknown named argument 'k' in call to 'WAuxLet'; 'WAuxLet' has no parameter with that name |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:4781` | 7 | WPubLet | diagnostic prose | k | — | — | `CtorUnknownField` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_UNKNOWN_FIELD: unknown named argument 'k' in call to 'WPubLet'; 'WPubLet' has no parameter with that name |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:4791` | 7 | WGeomLet | diagnostic prose | g | — | — | `CtorUnknownField` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_UNKNOWN_FIELD: unknown named argument 'g' in call to 'WGeomLet'; 'WGeomLet' has no parameter with that name |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:4802` | 8 | WSubMember | diagnostic prose | inner | — | — | `CtorUnknownField` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_UNKNOWN_FIELD: unknown named argument 'inner' in call to 'WSubMember'; 'WSubMember' has no parameter with that name |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:4865` | 4 | WRepeat | diagnostic prose | labl | — | — | `CtorUnknownField` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_UNKNOWN_FIELD: unknown named argument 'labl' in call to 'WRepeat'; 'WRepeat' has no parameter with that name |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:4865` | 4 | WRepeat | diagnostic prose | labl | — | — | `CtorUnknownField` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_UNKNOWN_FIELD: unknown named argument 'labl' in call to 'WRepeat'; 'WRepeat' has no parameter with that name |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:5139` | 2 | RayleighDamping | diagnostic prose | bta | — | — | `CtorUnknownField` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | E_CTOR_UNKNOWN_FIELD: unknown named argument 'bta' in call to 'RayleighDamping'; 'RayleighDamping' has no parameter with that name |
| `crates/reify-compiler/tests/harness_traits/trait_typed_param_tests.rs:314` | 4 | NotAMaterial | ctor call-site anchor | m | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'NotAMaterial' does not conform to trait 'MaterialSpec' required by param 'm' |
| `crates/reify-eval-fea-tests/tests/dynamics_compute_node.rs:41` | 2 | Frame3 | ctor call-site anchor | x_axis | Real | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'x_axis' has quantity 'Scalar[m]' but param 'x_axis' requires quantity 'Real' (the compared shape 'Vector3<Scalar[m]>' is otherwise accepted at 'Vector3<Real>'; only the quantity slot disagrees) |
| `crates/reify-eval-fea-tests/tests/dynamics_compute_node.rs:41` | 2 | Frame3 | ctor call-site anchor | y_axis | Real | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'y_axis' has quantity 'Scalar[m]' but param 'y_axis' requires quantity 'Real' (the compared shape 'Vector3<Scalar[m]>' is otherwise accepted at 'Vector3<Real>'; only the quantity slot disagrees) |
| `crates/reify-eval-fea-tests/tests/dynamics_compute_node.rs:41` | 2 | Frame3 | ctor call-site anchor | z_axis | Real | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'z_axis' has quantity 'Scalar[m]' but param 'z_axis' requires quantity 'Real' (the compared shape 'Vector3<Scalar[m]>' is otherwise accepted at 'Vector3<Real>'; only the quantity slot disagrees) |
| `crates/reify-eval-fea-tests/tests/r3b_modal_selector_displacement.rs:628` | 2 | RayleighDamping | ctor call-site anchor | alpha | Scalar[s^-1] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'alpha' has type 'Real' but param 'alpha' requires type 'Scalar[s^-1]'; pass a dimensioned Frequency literal |
| `crates/reify-eval-fea-tests/tests/r3b_modal_selector_displacement.rs:628` | 2 | RayleighDamping | ctor call-site anchor | beta | Scalar[s] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'beta' has type 'Real' but param 'beta' requires type 'Scalar[s]'; pass a dimensioned Time literal such as `1s` |
| `crates/reify-eval/tests/harness_dynamics/trajectory_gcode_dialect_eval.rs:195` | 7 | NotADialect | ctor call-site anchor | d | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'NotADialect' does not conform to trait 'GcodeDialect' required by param 'd' |
| `crates/reify-eval/tests/harness_engine/diagnostics_cache_replay_migration.rs:274` | 4 | Frame3 | ctor call-site anchor | x_axis | Real | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'x_axis' has quantity 'Scalar[m]' but param 'x_axis' requires quantity 'Real' (the compared shape 'Vector3<Scalar[m]>' is otherwise accepted at 'Vector3<Real>'; only the quantity slot disagrees) |
| `crates/reify-eval/tests/harness_engine/diagnostics_cache_replay_migration.rs:274` | 4 | Frame3 | ctor call-site anchor | y_axis | Real | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'y_axis' has quantity 'Scalar[m]' but param 'y_axis' requires quantity 'Real' (the compared shape 'Vector3<Scalar[m]>' is otherwise accepted at 'Vector3<Real>'; only the quantity slot disagrees) |
| `crates/reify-eval/tests/harness_engine/diagnostics_cache_replay_migration.rs:274` | 4 | Frame3 | ctor call-site anchor | z_axis | Real | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'z_axis' has quantity 'Scalar[m]' but param 'z_axis' requires quantity 'Real' (the compared shape 'Vector3<Scalar[m]>' is otherwise accepted at 'Vector3<Real>'; only the quantity slot disagrees) |
| `crates/reify-eval/tests/harness_mechanism/mechanism_modal_damping_e2e.rs:72` | 16 | RayleighDamping | ctor call-site anchor | alpha | Scalar[s^-1] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'alpha' has type 'Real' but param 'alpha' requires type 'Scalar[s^-1]'; pass a dimensioned Frequency literal |
| `crates/reify-eval/tests/harness_mechanism/mechanism_modal_damping_e2e.rs:72` | 16 | RayleighDamping | ctor call-site anchor | beta | Scalar[s] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'beta' has type 'Real' but param 'beta' requires type 'Scalar[s]'; pass a dimensioned Time literal such as `1s` |
| `crates/reify-eval/tests/pinned_support.rs:199` | 8 | NotASupport | ctor call-site anchor | sup | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'NotASupport' does not conform to trait 'Support' required by param 'sup' |
| `crates/reify-eval/tests/pressure_load.rs:197` | 8 | NotALoad | ctor call-site anchor | load | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'NotALoad' does not conform to trait 'Load' required by param 'load' |
| `crates/reify-eval/tests/structure_instance_e2e.rs:575` | 6 | Frame3 | ctor call-site anchor | x_axis | Real | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'x_axis' has quantity 'Scalar[m]' but param 'x_axis' requires quantity 'Real' (the compared shape 'Vector3<Scalar[m]>' is otherwise accepted at 'Vector3<Real>'; only the quantity slot disagrees) |
| `crates/reify-eval/tests/structure_instance_e2e.rs:575` | 6 | Frame3 | ctor call-site anchor | y_axis | Real | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'y_axis' has quantity 'Scalar[m]' but param 'y_axis' requires quantity 'Real' (the compared shape 'Vector3<Scalar[m]>' is otherwise accepted at 'Vector3<Real>'; only the quantity slot disagrees) |
| `crates/reify-eval/tests/structure_instance_e2e.rs:575` | 6 | Frame3 | ctor call-site anchor | z_axis | Real | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'z_axis' has quantity 'Scalar[m]' but param 'z_axis' requires quantity 'Real' (the compared shape 'Vector3<Scalar[m]>' is otherwise accepted at 'Vector3<Real>'; only the quantity slot disagrees) |
| `crates/reify-eval/tests/structure_instance_e2e.rs:729` | 6 | Frame3 | ctor call-site anchor | x_axis | Real | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'x_axis' has quantity 'Scalar[m]' but param 'x_axis' requires quantity 'Real' (the compared shape 'Vector3<Scalar[m]>' is otherwise accepted at 'Vector3<Real>'; only the quantity slot disagrees) |
| `crates/reify-eval/tests/structure_instance_e2e.rs:729` | 6 | Frame3 | ctor call-site anchor | y_axis | Real | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'y_axis' has quantity 'Scalar[m]' but param 'y_axis' requires quantity 'Real' (the compared shape 'Vector3<Scalar[m]>' is otherwise accepted at 'Vector3<Real>'; only the quantity slot disagrees) |
| `crates/reify-eval/tests/structure_instance_e2e.rs:729` | 6 | Frame3 | ctor call-site anchor | z_axis | Real | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'z_axis' has quantity 'Scalar[m]' but param 'z_axis' requires quantity 'Real' (the compared shape 'Vector3<Scalar[m]>' is otherwise accepted at 'Vector3<Real>'; only the quantity slot disagrees) |

### name recovered, but it is not a known structure def — needs manual triage — 62 site(s)

| site | snippet line | def | def source | field | expected | found | code | severity | hint (advisory) | disposition (γ ruling) | message |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `crates/reify-cli/tests/harness_cli/cli_check.rs:805` | 11 | mirror | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | mirror: ox argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-cli/tests/harness_cli/cli_check.rs:805` | 11 | mirror | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | mirror: oy argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-cli/tests/harness_cli/cli_check.rs:805` | 11 | mirror | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | mirror: oz argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_query_inline_arg_tests.rs:351` | 3 | box | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | box: depth argument expects Length, got Real; pass a dimensioned length such as `5mm` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_query_inline_arg_tests.rs:351` | 3 | box | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | box: height argument expects Length, got Real; pass a dimensioned length such as `5mm` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_query_inline_arg_tests.rs:351` | 3 | box | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | box: width argument expects Length, got Real; pass a dimensioned length such as `5mm` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_query_inline_arg_tests.rs:380` | 4 | box | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | box: depth argument expects Length, got Real; pass a dimensioned length such as `5mm` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_query_inline_arg_tests.rs:380` | 4 | box | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | box: height argument expects Length, got Real; pass a dimensioned length such as `5mm` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_query_inline_arg_tests.rs:380` | 4 | box | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | box: width argument expects Length, got Real; pass a dimensioned length such as `5mm` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_traits_inference_tests.rs:879` | 5 | intersection | ctor call-site anchor | g | — | — | `TypeNotConformingToTrait` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | geometry argument 'g' does not conform to trait 'Connected' |
| `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:1767` | 2 | translate | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | translate: dx argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:1767` | 2 | translate | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | translate: dy argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:1767` | 2 | translate | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | translate: dz argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:1858` | 2 | extrude | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | extrude: distance argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-compiler/tests/harness_langcore/type_hygiene_integration_gate.rs:208` | 3 | moment_of_inertia | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | moment_of_inertia: density argument expects Density, got Real; pass a dimensioned Density literal such as `7850kg/m^3` |
| `crates/reify-compiler/tests/harness_statement_semantics/generate_combinator_tests.rs:135` | 2 | generate | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | generate: n argument expects Int, got Scalar[m] |
| `crates/reify-compiler/tests/harness_statement_semantics/generate_combinator_tests.rs:157` | 2 | generate | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | generate: n argument expects Int, got Real |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:725` | 4 | box | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | box: width argument expects Length, got Widget; pass a dimensioned length such as `5mm` |
| `crates/reify-compiler/tests/harness_traits/trait_typed_param_tests.rs:425` | 6 | Rigid | ctor call-site anchor | m | Material | Rigid | `TypeNotConformingToStructureRef` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'm' has type 'Rigid' but param 'm' requires structure type 'Material' |
| `crates/reify-compiler/tests/harness_traits/trait_typed_param_tests.rs:538` | 4 | some | ctor call-site anchor | m | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'NotAMaterial' does not conform to trait 'MaterialSpec' required by param 'm' |
| `crates/reify-compiler/tests/harness_traits/trait_typed_param_tests.rs:1593` | 8 | some | ctor call-site anchor | ms | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'NotAMaterial' does not conform to trait 'MaterialSpec' required by param 'ms' |
| `crates/reify-compiler/tests/param_binding_selector_coercion_tests.rs:143` | 5 | needs_face | ctor call-site anchor | — | — | — | `SelectorKindMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | no matching overload for needs_face(EdgeSelector), candidates: needs_face(FaceSelector) -> Int |
| `crates/reify-compiler/tests/selector_composition_tests.rs:26` | 3 | union | ctor call-site anchor | — | — | — | `SelectorKindMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | selector composition kind mismatch: cannot compose FaceSelector and EdgeSelector |
| `crates/reify-compiler/tests/selector_composition_tests.rs:34` | 3 | intersect | ctor call-site anchor | — | — | — | `SelectorKindMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | selector composition kind mismatch: cannot compose FaceSelector and EdgeSelector |
| `crates/reify-compiler/tests/selector_composition_tests.rs:42` | 3 | difference | ctor call-site anchor | — | — | — | `SelectorKindMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | selector composition kind mismatch: cannot compose FaceSelector and EdgeSelector |
| `crates/reify-compiler/tests/selector_composition_tests.rs:479` | 5 | difference | ctor call-site anchor | — | — | — | `SelectorKindMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | selector `difference` requires exactly 2 operands, got 3 |
| `crates/reify-compiler/tests/selector_composition_tests.rs:785` | 3 | union | ctor call-site anchor | — | — | — | `SelectorKindMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | selector composition kind mismatch: cannot compose VertexSelector and FaceSelector |
| `crates/reify-eval/tests/harness_cache/unified_dag_geometry_executors.rs:916` | 6 | fillet | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | fillet: radius argument expects Length, got Real; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/geometry_length_args_units_e2e.rs:139` | 2 | translate | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | translate: dx argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/geometry_length_args_units_e2e.rs:139` | 2 | translate | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | translate: dy argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/geometry_length_args_units_e2e.rs:139` | 2 | translate | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | translate: dz argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/geometry_length_args_units_e2e.rs:157` | 2 | rotate_around | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | rotate_around: px argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/geometry_length_args_units_e2e.rs:157` | 2 | rotate_around | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | rotate_around: py argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/geometry_length_args_units_e2e.rs:157` | 2 | rotate_around | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | rotate_around: pz argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/geometry_length_args_units_e2e.rs:172` | 2 | revolve | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | revolve: ox argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/geometry_length_args_units_e2e.rs:172` | 2 | revolve | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | revolve: oy argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/geometry_length_args_units_e2e.rs:172` | 2 | revolve | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | revolve: oz argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/modify_sweep_length_units_e2e.rs:227` | 2 | fillet | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | fillet: radius argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/modify_sweep_length_units_e2e.rs:317` | 2 | fillet | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | fillet: radius argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/modify_sweep_length_units_e2e.rs:401` | 2 | chamfer | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | chamfer: distance argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/modify_sweep_length_units_e2e.rs:483` | 3 | chamfer_asymmetric | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | chamfer_asymmetric: d1 argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/modify_sweep_length_units_e2e.rs:483` | 3 | chamfer_asymmetric | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | chamfer_asymmetric: d2 argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/modify_sweep_length_units_e2e.rs:611` | 2 | extrude | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | extrude: distance argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/primitive_profile_length_units_e2e.rs:215` | 2 | box | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | box: depth argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/primitive_profile_length_units_e2e.rs:215` | 2 | box | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | box: height argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/primitive_profile_length_units_e2e.rs:215` | 2 | box | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | box: width argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/primitive_profile_length_units_e2e.rs:294` | 2 | box | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | box: depth argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/primitive_profile_length_units_e2e.rs:294` | 2 | box | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | box: height argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/primitive_profile_length_units_e2e.rs:294` | 2 | box | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | box: width argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/harness_geometry/primitive_profile_length_units_e2e.rs:314` | 2 | circle | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | circle: radius argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/mirror_circular_value_forms_e2e.rs:557` | 3 | circular_pattern | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | circular_pattern: ox argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/mirror_circular_value_forms_e2e.rs:557` | 3 | circular_pattern | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | circular_pattern: oy argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/mirror_circular_value_forms_e2e.rs:557` | 3 | circular_pattern | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | circular_pattern: oz argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/mirror_circular_value_forms_e2e.rs:834` | 3 | mirror | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | mirror: ox argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/mirror_circular_value_forms_e2e.rs:834` | 3 | mirror | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | mirror: oy argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/mirror_circular_value_forms_e2e.rs:834` | 3 | mirror | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | mirror: oz argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/pattern_spacing_units_e2e.rs:94` | 2 | linear_pattern_2d | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | linear_pattern_2d: spacing1 argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/pattern_spacing_units_e2e.rs:94` | 2 | linear_pattern_2d | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | linear_pattern_2d: spacing2 argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/pattern_spacing_units_e2e.rs:177` | 2 | linear_pattern | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | linear_pattern: spacing argument expects Length, got Int; pass a dimensioned length such as `5mm` |
| `crates/reify-eval/tests/region_resolution_boundary.rs:688` | 4 | needs_face | ctor call-site anchor | — | — | — | `SelectorKindMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | no matching overload for needs_face(EdgeSelector), candidates: needs_face(FaceSelector) -> Int |
| `crates/reify-eval/tests/region_resolution_boundary.rs:702` | 4 | needs_face | ctor call-site anchor | — | — | — | `SelectorKindMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | no matching overload for needs_face(BodySelector), candidates: needs_face(FaceSelector) -> Int |
| `crates/reify-eval/tests/type_hygiene_integration_gate.rs:312` | 3 | moment_of_inertia | ctor call-site anchor | — | — | — | `ArgTypeMismatch` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | moment_of_inertia: density argument expects Density, got Real; pass a dimensioned Density literal such as `7850kg/m^3` |

### unattributed def — needs manual triage — 81 site(s)

| site | snippet line | def | def source | field | expected | found | code | severity | hint (advisory) | disposition (γ ruling) | message |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs:1453` | 2 | — | unrecovered: identifier not followed by `(` | material | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'material' has type 'Real' but param 'material' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs:1454` | 3 | — | unrecovered: identifier not followed by `(` | youngs_modulus | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'youngs_modulus' has type 'Real' but param 'youngs_modulus' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs:1516` | 2 | — | unrecovered: identifier not followed by `(` | material | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'material' has type 'Real' but param 'material' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs:1517` | 3 | — | unrecovered: identifier not followed by `(` | youngs_modulus | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'youngs_modulus' has type 'Real' but param 'youngs_modulus' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs:1567` | 2 | — | unrecovered: identifier not followed by `(` | z | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'z' has type 'Real' but param 'z' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_traits_inference_tests.rs:916` | 6 | — | unrecovered: identifier not followed by `(` | g | — | — | `TypeNotConformingToTrait` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | geometry argument 'g' does not conform to trait 'Connected' |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_traits_inference_tests.rs:996` | 7 | — | unrecovered: identifier not followed by `(` | g | — | — | `TypeNotConformingToTrait` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | geometry argument 'g' does not conform to trait 'Connected' |
| `crates/reify-compiler/tests/harness_geometry_solver/solver_elastic_static_stdlib_compile.rs:238` | 2 | — | unrecovered: identifier not followed by `(` | loads | — | — | `TypeNotConformingToTrait` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'Steel_AISI_1045' does not conform to trait 'Load' required by param 'loads' |
| `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:2109` | 4 | — | unrecovered: identifier not followed by `(` | axis | Scalar[m] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'axis' has type 'Int' but param 'axis' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:2274` | 4 | — | unrecovered: identifier not followed by `(` | axis | Scalar[m] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'axis' has type 'Int' but param 'axis' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:2335` | 6 | — | unrecovered: identifier not followed by `(` | axis | Scalar[m] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'axis' has type 'Int' but param 'axis' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:2443` | 5 | — | unrecovered: identifier not followed by `(` | axis | Scalar[m] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'axis' has type 'Int' but param 'axis' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:2555` | 4 | — | unrecovered: identifier not followed by `(` | cond | Scalar[m] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'cond' has type 'Int' but param 'cond' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:2624` | 2 | — | unrecovered: identifier not followed by `(` | cond | Scalar[m] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'cond' has type 'Int' but param 'cond' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/harness_langcore/type_alias_compile_tests.rs:957` | 4 | — | unrecovered: identifier not followed by `(` | w | Scalar[m·kg·s^-2] | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'w' has type 'Scalar[m]' but param 'w' requires type 'Scalar[m·kg·s^-2]'; pass a dimensioned Force literal such as `1m*kg/s^2` |
| `crates/reify-compiler/tests/harness_mechanics/fea_supertrait_conformance_tests.rs:171` | 2 | — | unrecovered: identifier not followed by `(` | material | — | — | `TypeNotConformingToTrait` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'Real' does not conform to trait 'ConstitutiveLaw' required by param 'material' |
| `crates/reify-compiler/tests/harness_mechanics/fea_supertrait_conformance_tests.rs:229` | 2 | — | unrecovered: identifier not followed by `(` | material | — | — | `TypeNotConformingToTrait` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'ElasticOptions' does not conform to trait 'ConstitutiveLaw' required by param 'material' |
| `crates/reify-compiler/tests/harness_mechanics/modal_options_validation_tests.rs:2368` | 2 | — | unrecovered: identifier not followed by `(` | part | Part | String | `TypeNotConformingToStructureRef` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'part' has type 'String' but param 'part' requires structure type 'Part' |
| `crates/reify-compiler/tests/harness_modules_ports/prelude_context_tests.rs:217` | 2 | — | unrecovered: identifier not followed by `(` | x | Scalar[m] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'x' has type 'Int' but param 'x' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:207` | 6 | — | unrecovered: identifier not followed by `(` | m | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'Electrical' does not conform to trait 'Mechanical' required by param 'm' |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:244` | 5 | — | unrecovered: identifier not followed by `(` | m | Material | Scalar[m] | `TypeNotConformingToStructureRef` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'm' has type 'Scalar[m]' but param 'm' requires structure type 'Material' |
| `crates/reify-compiler/tests/harness_physical_modeling/solid_param_tests.rs:637` | 2 | — | unrecovered: identifier not followed by `(` | — | — | — | `TypeNotConformingToStructureRef` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | param 'g' has type 'Geometry' but its default expression has non-geometry type 'Int' |
| `crates/reify-compiler/tests/harness_structure_declarations/collection_sub_tests.rs:426` | 3 | — | unrecovered: identifier not followed by `(` | grade | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'grade' has type 'Real' but param 'grade' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/harness_structure_declarations/collection_sub_tests.rs:510` | 1 | — | unrecovered: identifier not followed by `(` | grade | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'grade' has type 'Real' but param 'grade' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:138` | 4 | — | unrecovered: label span starts at a non-identifier | label | String | Int | `ArgTypeMismatch` | Warning | string field given a non-string literal | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Int' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:217` | 4 | — | unrecovered: label span starts at a non-identifier | face | FaceSelector | Int | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'face' has type 'Int' but param 'face' requires selector type 'FaceSelector' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:600` | 2 | — | unrecovered: identifier not followed by `(` | label | String | Int | `ArgTypeMismatch` | Warning | string field given a non-string literal | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Int' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:705` | 4 | — | unrecovered: label span starts at a non-identifier | label | String | Int | `ArgTypeMismatch` | Warning | string field given a non-string literal | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'label' has type 'Int' but param 'label' requires type 'String' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:1559` | 7 | — | unrecovered: identifier not followed by `(` | r | Result<Scalar[m], String> | String | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'r' has type 'String' but param 'r' requires type 'Result<Scalar[m], String>' |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:1595` | 11 | — | unrecovered: identifier not followed by `(` | c | Enum(Hue) | Enum(Outline) | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'c' has type 'Enum(Outline)' but param 'c' requires type 'Enum(Hue)' |
| `crates/reify-compiler/tests/harness_traits/fn_arg_trait_conformance_tests.rs:225` | 15 | — | unrecovered: identifier not followed by `(` | joint | — | — | `TypeNotConformingToTrait` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'FixedThing' does not conform to trait 'DrivingJoint' required by param 'joint' |
| `crates/reify-compiler/tests/harness_traits/fn_arg_trait_conformance_tests.rs:375` | 6 | — | unrecovered: identifier not followed by `(` | joint | — | — | `TypeNotConformingToTrait` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'Real' does not conform to trait 'DrivingJoint' required by param 'joint' |
| `crates/reify-compiler/tests/harness_traits/fn_arg_trait_conformance_tests.rs:421` | 7 | — | unrecovered: identifier not followed by `(` | joint | — | — | `TypeNotConformingToTrait` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'Real' does not conform to trait 'DrivingJoint' required by param 'joint' |
| `crates/reify-compiler/tests/harness_traits/fn_param_struct_ctor_default_tests.rs:466` | 6 | — | unrecovered: identifier not followed by `(` | material | — | — | `TypeNotConformingToTrait` | Error | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'NotAMaterial' does not conform to trait 'ElasticMaterial' required by param 'material' |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_type_conformance_tests.rs:31` | 5 | — | unrecovered: identifier not followed by `(` | w | Scalar[m] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'w' has type 'Int' but param 'w' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_type_conformance_tests.rs:76` | 6 | — | unrecovered: identifier not followed by `(` | w | Scalar[m] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'w' has type 'Int' but param 'w' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_type_conformance_tests.rs:110` | 8 | — | unrecovered: identifier not followed by `(` | w | Scalar[m] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'w' has type 'Int' but param 'w' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_type_conformance_tests.rs:166` | 6 | — | unrecovered: identifier not followed by `(` | w | Scalar[m] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'w' has type 'Int' but param 'w' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_type_conformance_tests.rs:224` | 6 | — | unrecovered: identifier not followed by `(` | w | Scalar[m] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'w' has type 'Int' but param 'w' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/harness_traits/trait_typed_param_tests.rs:466` | 3 | — | unrecovered: label span starts at a non-identifier | m | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'Real' does not conform to trait 'MaterialSpec' required by param 'm' |
| `crates/reify-compiler/tests/harness_traits/trait_typed_param_tests.rs:625` | 8 | — | unrecovered: label span starts at a non-identifier | ms | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'NotAMaterial' does not conform to trait 'MaterialSpec' required by param 'ms' |
| `crates/reify-compiler/tests/harness_traits/trait_typed_param_tests.rs:712` | 8 | — | unrecovered: identifier not followed by `(` | ms | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'NotAMaterial' does not conform to trait 'MaterialSpec' required by param 'ms' |
| `crates/reify-compiler/tests/harness_traits/trait_typed_param_tests.rs:774` | 8 | — | unrecovered: identifier not followed by `(` | ms | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'NotAMaterial' does not conform to trait 'MaterialSpec' required by param 'ms' |
| `crates/reify-compiler/tests/harness_traits/trait_typed_param_tests.rs:1101` | 8 | — | unrecovered: label span starts at a non-identifier | ms | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'NotAMaterial' does not conform to trait 'MaterialSpec' required by param 'ms' |
| `crates/reify-compiler/tests/harness_traits/trait_typed_param_tests.rs:1181` | 6 | — | unrecovered: identifier not followed by `(` | ms | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'Inert' does not conform to trait 'Carrier' required by param 'ms' |
| `crates/reify-compiler/tests/harness_traits/trait_typed_param_tests.rs:1311` | 7 | — | unrecovered: label span starts at a non-identifier | m | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'List<Steel>' does not conform to trait 'MaterialSpec' required by param 'm' |
| `crates/reify-compiler/tests/harness_traits/trait_typed_param_tests.rs:1356` | 7 | — | unrecovered: identifier not followed by `(` | ms | List<MaterialSpec> | Map<String, Steel> | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'Map<String, Steel>' does not match wrapper shape required by param 'ms' (expected 'List<MaterialSpec>') |
| `crates/reify-compiler/tests/harness_traits/trait_typed_param_tests.rs:1401` | 5 | — | unrecovered: identifier not followed by `(` | m | Material | List<Material> | `TypeNotConformingToStructureRef` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'm' has type 'List<Material>' but param 'm' requires structure type 'Material' |
| `crates/reify-compiler/tests/harness_traits/trait_typed_param_tests.rs:1452` | 5 | — | unrecovered: identifier not followed by `(` | ms | Set<M> | List<M> | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'List<M>' does not match wrapper shape required by param 'ms' (expected 'Set<M>') |
| `crates/reify-compiler/tests/harness_traits/trait_typed_param_tests.rs:1497` | 5 | — | unrecovered: identifier not followed by `(` | ms | Map<String, M> | List<M> | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'List<M>' does not match wrapper shape required by param 'ms' (expected 'Map<String, M>') |
| `crates/reify-compiler/tests/harness_traits/trait_typed_param_tests.rs:1550` | 8 | — | unrecovered: identifier not followed by `(` | ms | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'NotAMaterial' does not conform to trait 'MaterialSpec' required by param 'ms' |
| `crates/reify-compiler/tests/harness_type_checking/polymorphic_zero_tests.rs:750` | 3 | — | unrecovered: identifier not followed by `(` | phase_bare | Scalar[rad] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'phase_bare' has type 'Int' but param 'phase_bare' requires type 'Scalar[rad]'; pass a dimensioned Angle literal such as `1rad` |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:30` | 4 | — | unrecovered: identifier not followed by `(` | drum_d | Real | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'drum_d' has type 'Scalar[m]' but param 'drum_d' requires type 'Real' |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:175` | 2 | — | unrecovered: identifier not followed by `(` | zero_int | Scalar[m] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'zero_int' has type 'Int' but param 'zero_int' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:176` | 3 | — | unrecovered: identifier not followed by `(` | one_int | Scalar[m] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'one_int' has type 'Int' but param 'one_int' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:177` | 4 | — | unrecovered: identifier not followed by `(` | half_real | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'half_real' has type 'Real' but param 'half_real' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:178` | 5 | — | unrecovered: identifier not followed by `(` | large_real | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'large_real' has type 'Real' but param 'large_real' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:206` | 2 | — | unrecovered: identifier not followed by `(` | neg_real | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'neg_real' has type 'Real' but param 'neg_real' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:207` | 3 | — | unrecovered: identifier not followed by `(` | neg_int | Scalar[m] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'neg_int' has type 'Int' but param 'neg_int' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:233` | 2 | — | unrecovered: identifier not followed by `(` | bad_mass | Scalar[m] | Scalar[kg] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'bad_mass' has type 'Scalar[kg]' but param 'bad_mass' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:301` | 3 | — | unrecovered: identifier not followed by `(` | c | Real | Enum(Color) | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'c' has type 'Enum(Color)' but param 'c' requires type 'Real' |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:325` | 2 | — | unrecovered: identifier not followed by `(` | x | Real | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'x' has type 'Scalar[m]' but param 'x' requires type 'Real' |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:369` | 4 | — | unrecovered: identifier not followed by `(` | bad_dim | Scalar[m] | Scalar[m^-1] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'bad_dim' has type 'Scalar[m^-1]' but param 'bad_dim' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:409` | 3 | — | unrecovered: identifier not followed by `(` | x | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'x' has type 'Real' but param 'x' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:441` | 2 | — | unrecovered: identifier not followed by `(` | x | Int | Scalar[kg] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'x' has type 'Scalar[kg]' but param 'x' requires type 'Int' |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:469` | 2 | — | unrecovered: identifier not followed by `(` | x | Int | Real | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'x' has type 'Real' but param 'x' requires type 'Int' |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:509` | 4 | — | unrecovered: identifier not followed by `(` | bad_dim | Real | Scalar[m^-1] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'bad_dim' has type 'Scalar[m^-1]' but param 'bad_dim' requires type 'Real' |
| `crates/reify-eval/tests/collection_sub_eval.rs:437` | 1 | — | unrecovered: identifier not followed by `(` | grade | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'grade' has type 'Real' but param 'grade' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-eval/tests/determinacy_predicates.rs:509` | 2 | — | unrecovered: identifier not followed by `(` | a | Scalar[m] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'a' has type 'Int' but param 'a' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-eval/tests/gravity_load.rs:453` | 8 | — | unrecovered: label span starts at a non-identifier | loads | — | — | `TypeNotConformingToTrait` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | type 'NotALoad' does not conform to trait 'Load' required by param 'loads' |
| `crates/reify-eval/tests/harness_engine/nested_sub_derived_let_e2e.rs:576` | 7 | — | unrecovered: identifier not followed by `(` | v | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'v' has type 'Real' but param 'v' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-eval/tests/harness_engine/nested_sub_derived_let_e2e.rs:708` | 9 | — | unrecovered: identifier not followed by `(` | w | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'w' has type 'Real' but param 'w' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-eval/tests/harness_engine/nested_sub_derived_let_e2e.rs:790` | 7 | — | unrecovered: identifier not followed by `(` | v | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'v' has type 'Real' but param 'v' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-eval/tests/harness_stress_scenarios/stress_error_messages.rs:126` | 2 | — | unrecovered: identifier not followed by `(` | x | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'x' has type 'Real' but param 'x' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-eval/tests/harness_stress_scenarios/stress_sweep_degenerate.rs:417` | 2 | — | unrecovered: identifier not followed by `(` | x | Scalar[m] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'x' has type 'Int' but param 'x' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-eval/tests/harness_stress_scenarios/stress_sweep_degenerate.rs:418` | 3 | — | unrecovered: identifier not followed by `(` | y | Scalar[m] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'y' has type 'Int' but param 'y' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-eval/tests/purpose_activation.rs:2673` | 2 | — | unrecovered: identifier not followed by `(` | material | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'material' has type 'Real' but param 'material' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-eval/tests/purpose_activation.rs:2674` | 3 | — | unrecovered: identifier not followed by `(` | youngs_modulus | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'youngs_modulus' has type 'Real' but param 'youngs_modulus' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-eval/tests/purpose_activation.rs:2774` | 2 | — | unrecovered: identifier not followed by `(` | z | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'z' has type 'Real' but param 'z' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-eval/tests/purpose_activation.rs:2825` | 2 | — | unrecovered: identifier not followed by `(` | z | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'z' has type 'Real' but param 'z' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `crates/reify-syntax/tests/harness_syntax/option_tests.rs:157` | 2 | — | unrecovered: identifier not followed by `(` | x | Real | Option<Real> | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | census — inline Rust fixture, sites owned by #5306: enumerated here, fixed there | argument 'x' has type 'Option<Real>' but param 'x' requires type 'Real' |

### Inline coverage

Of 3198 inline member(s) — one per extracted snippet, plus one per host that could not be read at all — **3115 were swept** and **83 were not**. A further **682** were swept only PARTIALLY. A member is keyed `<host>:<line>`, the host line the snippet's own line 1 sits on.

#### Not swept (contributed no sites)

| file | reason |
|---|---|
| `crates/reify-compiler/tests/ambient_default_injection_tests.rs:138` | `format-template` |
| `crates/reify-compiler/tests/ambient_default_injection_tests.rs:216` | `format-template` |
| `crates/reify-compiler/tests/ambient_default_injection_tests.rs:280` | `format-template` |
| `crates/reify-compiler/tests/ambient_default_injection_tests.rs:341` | `format-template` |
| `crates/reify-compiler/tests/ambient_default_injection_tests.rs:411` | `format-template` |
| `crates/reify-compiler/tests/ambient_default_material_integration_gate.rs:165` | `format-template` |
| `crates/reify-compiler/tests/ambient_default_material_integration_gate.rs:55` | `format-template` |
| `crates/reify-compiler/tests/ambient_default_material_integration_gate.rs:92` | `format-template` |
| `crates/reify-compiler/tests/guard_compilation.rs:722` | `parse-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/ctor_conformance_corpus_survey.rs:3198` | `format-template` |
| `crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs:537` | `format-template` |
| `crates/reify-compiler/tests/harness_constructor_typing/math_signatures.rs:662` | `format-template` |
| `crates/reify-compiler/tests/harness_constructor_typing/math_signatures.rs:696` | `format-template` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_traits_tests.rs:124` | `format-template` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_traits_tests.rs:208` | `format-template` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_traits_user_asserted_tests.rs:272` | `format-template` |
| `crates/reify-compiler/tests/harness_geometry_solver/solver_hint_tests.rs:300` | `format-template` |
| `crates/reify-compiler/tests/harness_langcore/priv_member_visibility_tests.rs:1023` | `format-template` |
| `crates/reify-compiler/tests/harness_langcore/priv_member_visibility_tests.rs:1085` | `format-template` |
| `crates/reify-compiler/tests/harness_langcore/priv_member_visibility_tests.rs:825` | `format-template` |
| `crates/reify-compiler/tests/harness_langcore/uniform_member_path_tests.rs:141` | `format-template` |
| `crates/reify-compiler/tests/harness_mechanics/ground_sugar_tests.rs:76` | `format-template` |
| `crates/reify-compiler/tests/harness_mechanics/trajectory_stdlib_compile.rs:1419` | `format-template` |
| `crates/reify-compiler/tests/harness_mechanics/trajectory_stdlib_compile.rs:2029` | `format-template` |
| `crates/reify-compiler/tests/harness_modules_ports/connect_compile_tests.rs:1638` | `parse-error` |
| `crates/reify-compiler/tests/harness_modules_ports/sub_placement_lowering_tests.rs:474` | `parse-error` |
| `crates/reify-compiler/tests/harness_patterns/enum_unknown_type_param_tests.rs:190` | `format-template` |
| `crates/reify-compiler/tests/harness_patterns/generic_enum_pattern_binder_tests.rs:190` | `format-template` |
| `crates/reify-compiler/tests/harness_patterns/generic_enum_pattern_binder_tests.rs:220` | `format-template` |
| `crates/reify-compiler/tests/harness_patterns/generic_enum_pattern_binder_tests.rs:309` | `format-template` |
| `crates/reify-compiler/tests/harness_patterns/generic_enum_pattern_binder_tests.rs:350` | `format-template` |
| `crates/reify-compiler/tests/harness_physical_modeling/process_stdlib_compile.rs:799` | `format-template` |
| `crates/reify-compiler/tests/harness_statement_semantics/forall_statement_lower_tests.rs:142` | `format-template` |
| `crates/reify-compiler/tests/harness_statement_semantics/string_interp_lowering_tests.rs:136` | `format-template` |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:5028` | `format-template` |
| `crates/reify-compiler/tests/harness_traits/fn_arg_trait_conformance_tests.rs:100` | `format-template` |
| `crates/reify-compiler/tests/harness_traits/fn_arg_trait_conformance_tests.rs:136` | `format-template` |
| `crates/reify-compiler/tests/harness_traits/fn_arg_trait_conformance_tests.rs:171` | `format-template` |
| `crates/reify-compiler/tests/harness_traits/fn_arg_trait_conformance_tests.rs:512` | `format-template` |
| `crates/reify-compiler/tests/harness_traits/fn_arg_trait_conformance_tests.rs:56` | `format-template` |
| `crates/reify-compiler/tests/harness_traits/fn_arg_trait_conformance_tests.rs:577` | `format-template` |
| `crates/reify-compiler/tests/harness_traits/fn_arg_trait_conformance_tests.rs:614` | `format-template` |
| `crates/reify-compiler/tests/harness_traits/fn_arg_trait_conformance_tests.rs:646` | `format-template` |
| `crates/reify-compiler/tests/harness_traits/fn_arg_trait_conformance_tests.rs:674` | `format-template` |
| `crates/reify-compiler/tests/harness_traits/fn_arg_trait_conformance_tests.rs:735` | `format-template` |
| `crates/reify-compiler/tests/harness_traits/fn_arg_trait_conformance_tests.rs:780` | `format-template` |
| `crates/reify-compiler/tests/harness_traits/trait_arg_conformance_bench.rs:44` | `format-template` |
| `crates/reify-compiler/tests/harness_type_checking/add_sub_operand_guard_tests.rs:407` | `format-template` |
| `crates/reify-compiler/tests/harness_type_checking/add_sub_operand_guard_tests.rs:63` | `format-template` |
| `crates/reify-compiler/tests/harness_type_checking/and_or_operand_guard_tests.rs:39` | `format-template` |
| `crates/reify-compiler/tests/harness_type_checking/and_or_operand_guard_tests.rs:57` | `format-template` |
| `crates/reify-compiler/tests/harness_type_checking/expr_error_sentinel_tests.rs:592` | `format-template` |
| `crates/reify-compiler/tests/harness_type_checking/mul_div_operand_guard_tests.rs:256` | `parse-error` |
| `crates/reify-compiler/tests/harness_type_checking/mul_div_operand_guard_tests.rs:39` | `format-template` |
| `crates/reify-compiler/tests/harness_type_checking/mul_div_operand_guard_tests.rs:57` | `format-template` |
| `crates/reify-compiler/tests/harness_type_checking/polymorphic_zero_tests.rs:312` | `format-template` |
| `crates/reify-compiler/tests/harness_type_checking/polymorphic_zero_tests.rs:354` | `format-template` |
| `crates/reify-compiler/tests/harness_type_checking/polymorphic_zero_tests.rs:375` | `format-template` |
| `crates/reify-compiler/tests/silent_defaults_tests.rs:680` | `parse-error` |
| `crates/reify-eval-fea-tests/tests/r3b_modal_selector_displacement.rs:480` | `format-template` |
| `crates/reify-eval/tests/circular_pattern_angle.rs:23` | `format-template` |
| `crates/reify-eval/tests/cost_subtree_aggregate_eval.rs:135` | `format-template` |
| `crates/reify-eval/tests/cost_subtree_aggregate_eval.rs:86` | `format-template` |
| `crates/reify-eval/tests/dfm_fits_build_volume_e2e.rs:119` | `format-template` |
| `crates/reify-eval/tests/edit_source.rs:185` | `format-template` |
| `crates/reify-eval/tests/edit_source.rs:442` | `format-template` |
| `crates/reify-eval/tests/harness_auto_resolution/auto_sub_override_resolution.rs:61` | `format-template` |
| `crates/reify-eval/tests/harness_engine/joint_drive_cluster_formation.rs:86` | `format-template` |
| `crates/reify-eval/tests/harness_fea_solver_e2e/edit_path_optimized_dispatch.rs:141` | `parse-error` |
| `crates/reify-eval/tests/harness_geometry/rounded_corner_runtime_constraint.rs:32` | `format-template` |
| `crates/reify-eval/tests/harness_geometry/rounded_corner_runtime_constraint.rs:384` | `format-template` |
| `crates/reify-eval/tests/harness_modal/modal_material_damping_e2e.rs:524` | `format-template` |
| `crates/reify-eval/tests/joint_drive_expansion_boundary.rs:206` | `format-template` |
| `crates/reify-eval/tests/relate_solve_e2e.rs:300` | `format-template` |
| `crates/reify-syntax/tests/harness_syntax/boundary1_producer.rs:373` | `parse-error` |
| `crates/reify-syntax/tests/harness_syntax/boundary1_producer.rs:466` | `parse-error` |
| `crates/reify-syntax/tests/harness_syntax/boundary1_producer.rs:47` | `parse-error` |
| `crates/reify-syntax/tests/harness_syntax/interpolated_string_tests.rs:204` | `format-template` |
| `crates/reify-syntax/tests/harness_syntax/interpolated_string_tests.rs:277` | `parse-error` |
| `crates/reify-syntax/tests/harness_syntax/interpolated_string_tests.rs:58` | `format-template` |
| `crates/reify-test-support/tests/rust_fixture_scan.rs:46` | `parse-error` |
| `crates/reify-test-support/tests/rust_fixture_scan.rs:465` | `format-template` |
| `crates/reify-test-support/tests/rust_fixture_scan.rs:95` | `parse-error` |

#### Partially swept (sites collected, but the snippet also failed to compile)

| file | reason |
|---|---|
| `crates/reify-cli/tests/harness_cli/cli_check.rs:795` | `compile-error` |
| `crates/reify-compiler/tests/ambient_default_injection_tests.rs:188` | `compile-error` |
| `crates/reify-compiler/tests/ambient_default_injection_tests.rs:370` | `compile-error` |
| `crates/reify-compiler/tests/boundary2_producer.rs:1003` | `compile-error` |
| `crates/reify-compiler/tests/boundary2_producer.rs:1668` | `compile-error` |
| `crates/reify-compiler/tests/deep_dot_chain_tests.rs:116` | `compile-error` |
| `crates/reify-compiler/tests/deep_dot_chain_tests.rs:141` | `compile-error` |
| `crates/reify-compiler/tests/deep_dot_chain_tests.rs:188` | `compile-error` |
| `crates/reify-compiler/tests/deep_dot_chain_tests.rs:241` | `compile-error` |
| `crates/reify-compiler/tests/deep_dot_chain_tests.rs:277` | `compile-error` |
| `crates/reify-compiler/tests/deep_dot_chain_tests.rs:348` | `compile-error` |
| `crates/reify-compiler/tests/deep_dot_chain_tests.rs:359` | `compile-error` |
| `crates/reify-compiler/tests/deep_dot_chain_tests.rs:36` | `compile-error` |
| `crates/reify-compiler/tests/deep_dot_chain_tests.rs:370` | `compile-error` |
| `crates/reify-compiler/tests/deep_dot_chain_tests.rs:381` | `compile-error` |
| `crates/reify-compiler/tests/deep_dot_chain_tests.rs:393` | `compile-error` |
| `crates/reify-compiler/tests/deep_dot_chain_tests.rs:404` | `compile-error` |
| `crates/reify-compiler/tests/deep_dot_chain_tests.rs:415` | `compile-error` |
| `crates/reify-compiler/tests/deep_dot_chain_tests.rs:432` | `compile-error` |
| `crates/reify-compiler/tests/deep_dot_chain_tests.rs:445` | `compile-error` |
| `crates/reify-compiler/tests/deep_dot_chain_tests.rs:460` | `compile-error` |
| `crates/reify-compiler/tests/deep_dot_chain_tests.rs:483` | `compile-error` |
| `crates/reify-compiler/tests/deep_dot_chain_tests.rs:86` | `compile-error` |
| `crates/reify-compiler/tests/determinacy_compile_tests.rs:112` | `compile-error` |
| `crates/reify-compiler/tests/determinacy_compile_tests.rs:148` | `compile-error` |
| `crates/reify-compiler/tests/determinacy_compile_tests.rs:78` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:1017` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:1149` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:1185` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:1518` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:1556` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:1593` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:1634` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:1676` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:1722` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:188` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:225` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:264` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:305` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:345` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:388` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:439` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:489` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:542` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:581` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:624` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:668` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:706` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:744` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:784` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:824` | `compile-error` |
| `crates/reify-compiler/tests/diagnostic_coverage_checkpoint.rs:865` | `compile-error` |
| `crates/reify-compiler/tests/guard_compilation.rs:497` | `compile-error` |
| `crates/reify-compiler/tests/guard_compilation.rs:532` | `compile-error` |
| `crates/reify-compiler/tests/guard_compilation.rs:568` | `compile-error` |
| `crates/reify-compiler/tests/harness_auto_binding/auto_binding_sites_remaining_tests.rs:476` | `compile-error` |
| `crates/reify-compiler/tests/harness_auto_binding/auto_binding_sites_remaining_tests.rs:516` | `compile-error` |
| `crates/reify-compiler/tests/harness_auto_binding/auto_type_arg_lowering_tests.rs:111` | `compile-error` |
| `crates/reify-compiler/tests/harness_auto_binding/auto_type_arg_lowering_tests.rs:377` | `compile-error` |
| `crates/reify-compiler/tests/harness_auto_binding/auto_type_arg_lowering_tests.rs:73` | `compile-error` |
| `crates/reify-compiler/tests/harness_auto_binding/auto_type_param_member_access_tests.rs:267` | `compile-error` |
| `crates/reify-compiler/tests/harness_auto_binding/auto_type_param_member_access_tests.rs:359` | `compile-error` |
| `crates/reify-compiler/tests/harness_auto_binding/auto_type_param_monomorphize_tests.rs:1332` | `compile-error` |
| `crates/reify-compiler/tests/harness_auto_binding/auto_type_param_monomorphize_tests.rs:1422` | `compile-error` |
| `crates/reify-compiler/tests/harness_auto_binding/auto_type_param_monomorphize_tests.rs:1451` | `compile-error` |
| `crates/reify-compiler/tests/harness_auto_binding/auto_type_param_monomorphize_tests.rs:684` | `compile-error` |
| `crates/reify-compiler/tests/harness_auto_binding/auto_type_param_monomorphize_tests.rs:768` | `compile-error` |
| `crates/reify-compiler/tests/harness_auto_binding/auto_type_param_monomorphize_tests.rs:836` | `compile-error` |
| `crates/reify-compiler/tests/harness_auto_binding/auto_type_param_monomorphize_tests.rs:934` | `compile-error` |
| `crates/reify-compiler/tests/harness_auto_binding/auto_type_params_max_depth_config.rs:30` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/compile_api_tests.rs:1003` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/compile_api_tests.rs:1206` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/compile_api_tests.rs:1275` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/compile_api_tests.rs:1295` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/compile_api_tests.rs:1344` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/compile_api_tests.rs:1431` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/compile_api_tests.rs:1572` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/compile_api_tests.rs:1612` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/compile_api_tests.rs:1647` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/compile_api_tests.rs:1670` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/compile_api_tests.rs:1693` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/compile_api_tests.rs:1716` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/compile_api_tests.rs:1739` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/compile_api_tests.rs:286` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/compile_api_tests.rs:479` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/compile_api_tests.rs:499` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/compile_api_tests.rs:969` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/meta_compile_tests.rs:103` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/meta_compile_tests.rs:129` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/meta_compile_tests.rs:162` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/meta_compile_tests.rs:217` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/meta_compile_tests.rs:397` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/meta_compile_tests.rs:433` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/meta_compile_tests.rs:75` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/pragma_compile_tests.rs:2905` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs:105` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs:1515` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs:2179` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs:2231` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs:2271` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs:2315` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs:2358` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs:2401` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs:242` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs:359` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs:404` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs:440` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs:486` | `compile-error` |
| `crates/reify-compiler/tests/harness_compilation_surface/purpose_compile_tests.rs:857` | `compile-error` |
| `crates/reify-compiler/tests/harness_doc_chunks/geometry_chunk_smoke.rs:1981` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:105` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:120` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:136` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:151` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:166` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:179` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:192` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:207` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:220` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:239` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:252` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:265` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:278` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:294` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:309` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:322` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:337` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:350` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:363` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:376` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:391` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:404` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:417` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:432` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:445` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:458` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:60` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:75` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_arg_count_span_tests.rs:90` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_centered_primitives_tests.rs:146` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_centered_primitives_tests.rs:438` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_query_inline_arg_tests.rs:349` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_query_inline_arg_tests.rs:377` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_traits_inference_tests.rs:1735` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_traits_inference_tests.rs:1770` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_traits_inference_tests.rs:875` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_traits_inference_tests.rs:911` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/geometry_traits_inference_tests.rs:990` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/solver_elastic_static_stdlib_compile.rs:237` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/solver_hint_payload_tests.rs:123` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/structural_physical_tests.rs:1556` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/structural_physical_tests.rs:790` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/structural_query_compile_tests.rs:140` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/structural_query_filter_compile_tests.rs:147` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/structural_query_filter_compile_tests.rs:194` | `compile-error` |
| `crates/reify-compiler/tests/harness_geometry_solver/structural_query_filter_compile_tests.rs:96` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_annotation_type_mismatch_tests.rs:203` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_annotation_type_mismatch_tests.rs:236` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_annotation_type_mismatch_tests.rs:261` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_annotation_type_mismatch_tests.rs:288` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_annotation_type_mismatch_tests.rs:29` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_annotation_type_mismatch_tests.rs:318` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_annotation_type_mismatch_tests.rs:419` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_annotation_type_mismatch_tests.rs:84` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:1637` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:1707` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:1729` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:1766` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:1857` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:2106` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:2191` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:2552` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:2623` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:384` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_scope_tests.rs:445` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_type_disambiguation_tests.rs:145` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_type_disambiguation_tests.rs:239` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_type_disambiguation_tests.rs:286` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_type_disambiguation_tests.rs:687` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_type_disambiguation_tests.rs:750` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/let_type_disambiguation_tests.rs:913` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/parametric_alias_def_site_validation_tests.rs:113` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/parametric_field_resolution_tests.rs:251` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/parametric_field_resolution_tests.rs:278` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/parametric_vector_point_resolution_tests.rs:297` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/prelude_sub_member_typing_tests.rs:287` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/prelude_sub_member_typing_tests.rs:479` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/priv_member_visibility_tests.rs:1168` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/priv_member_visibility_tests.rs:1217` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/priv_member_visibility_tests.rs:1304` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/priv_member_visibility_tests.rs:1343` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/priv_member_visibility_tests.rs:1380` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/priv_member_visibility_tests.rs:1416` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/priv_member_visibility_tests.rs:195` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/priv_member_visibility_tests.rs:262` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/priv_member_visibility_tests.rs:328` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/priv_member_visibility_tests.rs:398` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/priv_member_visibility_tests.rs:454` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/priv_member_visibility_tests.rs:922` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/priv_member_visibility_tests.rs:982` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/priv_redundant_tests.rs:48` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/priv_redundant_tests.rs:90` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/type_error_propagation_tests.rs:114` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/type_error_propagation_tests.rs:146` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/type_error_propagation_tests.rs:177` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/type_error_propagation_tests.rs:255` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/type_error_propagation_tests.rs:301` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/type_hygiene_integration_gate.rs:106` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/type_hygiene_integration_gate.rs:139` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/type_hygiene_integration_gate.rs:206` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/type_hygiene_integration_gate.rs:252` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/type_hygiene_integration_gate.rs:58` | `compile-error` |
| `crates/reify-compiler/tests/harness_langcore/uniform_member_path_tests.rs:112` | `compile-error` |
| `crates/reify-compiler/tests/harness_mechanics/coupling_motionvalue_integration_gate.rs:198` | `compile-error` |
| `crates/reify-compiler/tests/harness_mechanics/dynamics_stdlib_compile.rs:696` | `compile-error` |
| `crates/reify-compiler/tests/harness_mechanics/dynamics_stdlib_compile.rs:733` | `compile-error` |
| `crates/reify-compiler/tests/harness_mechanics/dynamics_stdlib_compile.rs:765` | `compile-error` |
| `crates/reify-compiler/tests/harness_mechanics/dynamics_stdlib_compile.rs:774` | `compile-error` |
| `crates/reify-compiler/tests/harness_mechanics/dynamics_stdlib_compile.rs:783` | `compile-error` |
| `crates/reify-compiler/tests/harness_mechanics/fea_supertrait_conformance_tests.rs:170` | `compile-error` |
| `crates/reify-compiler/tests/harness_mechanics/fea_supertrait_conformance_tests.rs:228` | `compile-error` |
| `crates/reify-compiler/tests/harness_mechanics/mechanism_nondriving_joint_compile.rs:145` | `compile-error` |
| `crates/reify-compiler/tests/harness_mechanics/mechanism_nondriving_joint_compile.rs:211` | `compile-error` |
| `crates/reify-compiler/tests/harness_mechanics/mechanism_nondriving_joint_compile.rs:274` | `compile-error` |
| `crates/reify-compiler/tests/harness_mechanics/mechanism_nondriving_joint_compile.rs:311` | `compile-error` |
| `crates/reify-compiler/tests/harness_mechanics/mechanism_nondriving_joint_compile.rs:342` | `compile-error` |
| `crates/reify-compiler/tests/harness_mechanics/mechanism_nondriving_joint_compile.rs:374` | `compile-error` |
| `crates/reify-compiler/tests/harness_mechanics/mechanism_nondriving_joint_compile.rs:45` | `compile-error` |
| `crates/reify-compiler/tests/harness_mechanics/mechanism_nondriving_joint_compile.rs:79` | `compile-error` |
| `crates/reify-compiler/tests/harness_mechanics/modal_options_validation_tests.rs:507` | `compile-error` |
| `crates/reify-compiler/tests/harness_mechanics/trajectory_stdlib_compile.rs:3356` | `compile-error` |
| `crates/reify-compiler/tests/harness_mechanics/trajectory_stdlib_compile.rs:3386` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/connect_compile_tests.rs:157` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/connect_compile_tests.rs:195` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/connect_compile_tests.rs:1989` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/connect_compile_tests.rs:2020` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/connect_compile_tests.rs:2051` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/connect_compile_tests.rs:2107` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/connect_compile_tests.rs:2253` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/connect_compile_tests.rs:264` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/connect_compile_tests.rs:292` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/connect_compile_tests.rs:319` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/connect_compile_tests.rs:345` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/connect_compile_tests.rs:370` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/connect_compile_tests.rs:399` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/connect_compile_tests.rs:550` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/connect_compile_tests.rs:743` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/connect_compile_tests.rs:769` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/connect_compile_tests.rs:818` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/entity_overload_tests.rs:116` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/entity_overload_tests.rs:17` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/entity_overload_tests.rs:181` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/entity_overload_tests.rs:228` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/entity_overload_tests.rs:274` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/entity_overload_tests.rs:67` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/port_compile_tests.rs:383` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/ports_stdlib_compile.rs:2590` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/ports_stdlib_compile.rs:791` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/qualified_access_compile_tests.rs:131` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/qualified_access_compile_tests.rs:171` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/qualified_access_compile_tests.rs:252` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/sub_placement_lowering_tests.rs:207` | `compile-error` |
| `crates/reify-compiler/tests/harness_modules_ports/task_1570_tests.rs:246` | `compile-error` |
| `crates/reify-compiler/tests/harness_patterns/enum_unknown_type_param_tests.rs:283` | `compile-error` |
| `crates/reify-compiler/tests/harness_patterns/enum_unknown_type_param_tests.rs:57` | `compile-error` |
| `crates/reify-compiler/tests/harness_patterns/generic_enum_pattern_binder_tests.rs:276` | `compile-error` |
| `crates/reify-compiler/tests/harness_patterns/generic_enum_pattern_binder_tests.rs:386` | `compile-error` |
| `crates/reify-compiler/tests/harness_patterns/match_block_decl_lowering_tests.rs:219` | `compile-error` |
| `crates/reify-compiler/tests/harness_patterns/match_block_decl_lowering_tests.rs:268` | `compile-error` |
| `crates/reify-compiler/tests/harness_patterns/match_compile_tests.rs:59` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/half_space_compile_tests.rs:92` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:1019` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:1130` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:1166` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:1208` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:1247` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:128` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:275` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:305` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:348` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:398` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:446` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:495` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:532` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:573` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:617` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:653` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:66` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:688` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:726` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:762` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:799` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:835` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/m9_error_cases.rs:97` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/physical_constants_tests.rs:653` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/process_stdlib_compile.rs:380` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/process_stdlib_compile.rs:682` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/process_stdlib_compile.rs:830` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/rounded_primitives_tests.rs:1101` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/rounded_primitives_tests.rs:491` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/rounded_primitives_tests.rs:501` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/rounded_primitives_tests.rs:538` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/rounded_primitives_tests.rs:560` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/rounded_primitives_tests.rs:588` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/rounded_primitives_tests.rs:893` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/rounded_primitives_tests.rs:903` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/rounded_primitives_tests.rs:918` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/rounded_primitives_tests.rs:933` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/rounded_primitives_tests.rs:989` | `compile-error` |
| `crates/reify-compiler/tests/harness_physical_modeling/solid_param_tests.rs:983` | `compile-error` |
| `crates/reify-compiler/tests/harness_relate/relate_threading_tests.rs:171` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/cross_sub_geometry_diagnostic_tests.rs:240` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/cross_sub_geometry_diagnostic_tests.rs:300` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/cross_sub_geometry_diagnostic_tests.rs:368` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/ds_sentinel_l0_poison_tests.rs:117` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/ds_sentinel_l0_poison_tests.rs:67` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/ds_sentinel_l5_boundary_tests.rs:139` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/ds_sentinel_l5_boundary_tests.rs:295` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/ds_sentinel_l5_boundary_tests.rs:345` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/ds_sentinel_l5_boundary_tests.rs:472` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/expected_type_pushdown_integration.rs:326` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/expected_type_pushdown_integration.rs:351` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/expected_type_pushdown_let_tests.rs:142` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/expected_type_pushdown_let_tests.rs:171` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/objective_conflict.rs:67` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/objective_dimension_coherence.rs:61` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/result_combinator_overload_tests.rs:97` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/result_combinator_resolution_tests.rs:347` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/result_fallback_resolution_tests.rs:115` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/result_match_binder_tests.rs:111` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/result_match_binder_tests.rs:199` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/result_match_binder_tests.rs:236` | `compile-error` |
| `crates/reify-compiler/tests/harness_result_annotation/result_match_binder_tests.rs:77` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/ad_hoc_selector_compile_tests.rs:11` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/ad_hoc_selector_compile_tests.rs:118` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/ad_hoc_selector_compile_tests.rs:200` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/ad_hoc_selector_compile_tests.rs:260` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/ad_hoc_selector_compile_tests.rs:293` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/analysis_stress_fn_compile.rs:339` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/analysis_stress_fn_compile.rs:362` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/constraint_def_compile_tests.rs:1283` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/constraint_def_compile_tests.rs:1384` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/constraint_def_compile_tests.rs:1449` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/constraint_def_compile_tests.rs:363` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/constraint_def_compile_tests.rs:404` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/constraint_def_compile_tests.rs:446` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/constraint_def_compile_tests.rs:642` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/constraint_inst_tests.rs:239` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/constraint_inst_tests.rs:266` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/constraint_inst_tests.rs:297` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/display_annotation_tests.rs:161` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/display_annotation_tests.rs:91` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/forall_statement_lower_tests.rs:1963` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/forall_statement_stub_tests.rs:36` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/generate_combinator_tests.rs:134` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/generate_combinator_tests.rs:156` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/index_access_selector_coercion_tests.rs:113` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/keyed_sub_resolution_tests.rs:206` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/keyed_sub_resolution_tests.rs:250` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/keyed_sub_resolution_tests.rs:272` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/keyed_sub_resolution_tests.rs:329` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/keyed_sub_resolution_tests.rs:368` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/keyed_sub_resolution_tests.rs:417` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/keyed_sub_resolution_tests.rs:447` | `compile-error` |
| `crates/reify-compiler/tests/harness_statement_semantics/keyed_sub_resolution_tests.rs:485` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/collection_sub_tests.rs:750` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/field_compile_tests.rs:438` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/option_compile_tests.rs:220` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/option_compile_tests.rs:246` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/option_recovery_resolution_tests.rs:217` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/option_recovery_resolution_tests.rs:380` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/option_recovery_resolution_tests.rs:420` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/option_recovery_resolution_tests.rs:466` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/recursive_detection_tests.rs:461` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/recursive_detection_tests.rs:509` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/recursive_structure_tests.rs:218` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/shadowing_warning_tests.rs:1465` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:2026` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:3515` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:4123` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:4908` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:712` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:722` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs:748` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/structure_in_purpose_ambient_tests.rs:112` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/structure_in_purpose_ambient_tests.rs:155` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/structure_in_purpose_ambient_tests.rs:211` | `compile-error` |
| `crates/reify-compiler/tests/harness_structure_declarations/structure_in_purpose_ambient_tests.rs:31` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/assoc_type_projection_reduction_tests.rs:187` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/assoc_type_projection_reduction_tests.rs:277` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/assoc_type_projection_reduction_tests.rs:338` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/assoc_type_projection_reduction_tests.rs:503` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/assoc_type_projection_reduction_tests.rs:544` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/assoc_type_projection_reduction_tests.rs:597` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/fn_arg_trait_conformance_tests.rs:211` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/fn_arg_trait_conformance_tests.rs:370` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/fn_arg_trait_conformance_tests.rs:415` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/fn_generic_body_permissive_tests.rs:121` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/fn_generic_trait_bound_tests.rs:149` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/fn_generic_trait_bound_tests.rs:203` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/fn_generic_trait_bound_tests.rs:23` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/fn_generic_trait_bound_tests.rs:238` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/fn_generic_trait_bound_tests.rs:308` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/fn_overload_tests.rs:14` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/fn_overload_tests.rs:206` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/fn_param_default_consumption_tests.rs:293` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/fn_param_default_consumption_tests.rs:99` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/fn_param_struct_ctor_default_tests.rs:461` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_fn_conformance_tests.rs:25` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_fn_instance_tests.rs:112` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_fn_instance_tests.rs:398` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_fn_instance_tests.rs:486` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_fn_overload_tests.rs:199` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_fn_overload_tests.rs:238` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_fn_overload_tests.rs:395` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_fn_overload_tests.rs:630` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_fn_static_tests.rs:171` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_fn_static_tests.rs:234` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_fn_static_tests.rs:273` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_fn_static_tests.rs:80` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_fn_structure_override_tests.rs:131` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_type_conformance_tests.rs:219` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_type_conformance_tests.rs:27` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_type_qualified_resolution_tests.rs:223` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_type_qualified_resolution_tests.rs:254` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_type_qualified_resolution_tests.rs:295` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_type_qualified_resolution_tests.rs:455` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_type_qualified_resolution_tests.rs:93` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_type_resolution_tests.rs:111` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_assoc_type_resolution_tests.rs:200` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_body_deferred_check_tests.rs:127` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_body_deferred_check_tests.rs:155` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_body_deferred_check_tests.rs:206` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_body_deferred_check_tests.rs:235` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_body_deferred_check_tests.rs:53` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_bounds_tests.rs:120` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_bounds_tests.rs:149` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_bounds_tests.rs:217` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_bounds_tests.rs:276` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_bounds_tests.rs:372` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_bounds_tests.rs:511` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_bounds_tests.rs:542` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_bounds_tests.rs:628` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_conformance_tests.rs:1078` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_conformance_tests.rs:1132` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_conformance_tests.rs:1183` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_conformance_tests.rs:132` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_conformance_tests.rs:164` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_conformance_tests.rs:313` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_conformance_tests.rs:456` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_conformance_tests.rs:638` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_conformance_tests.rs:922` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_conformance_type_error_tests.rs:108` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_conformance_type_error_tests.rs:136` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_conformance_type_error_tests.rs:198` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_conformance_type_error_tests.rs:221` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_conformance_type_error_tests.rs:254` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_default_collision_tests.rs:186` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_default_collision_tests.rs:248` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_default_collision_tests.rs:392` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_default_collision_tests.rs:456` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_default_collision_tests.rs:52` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_default_collision_tests.rs:83` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_merge_tests.rs:110` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_merge_tests.rs:1484` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_merge_tests.rs:214` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_merge_tests.rs:254` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_merge_tests.rs:296` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_merge_tests.rs:334` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_merge_tests.rs:69` | `compile-error` |
| `crates/reify-compiler/tests/harness_traits/trait_typed_param_tests.rs:237` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/boolean_arg_cross_sub_diagnostic_tests.rs:159` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/boolean_arg_cross_sub_diagnostic_tests.rs:187` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/boolean_arg_cross_sub_diagnostic_tests.rs:223` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/boolean_arg_cross_sub_diagnostic_tests.rs:260` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/boolean_arg_cross_sub_diagnostic_tests.rs:300` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/comparison_operand_guard_tests.rs:122` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/comparison_operand_guard_tests.rs:154` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/comparison_operand_guard_tests.rs:171` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/comparison_operand_guard_tests.rs:188` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/comparison_operand_guard_tests.rs:206` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/comparison_operand_guard_tests.rs:231` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/comparison_operand_guard_tests.rs:434` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/comparison_operand_guard_tests.rs:472` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/comparison_operand_guard_tests.rs:548` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/comparison_operand_guard_tests.rs:576` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/comparison_operand_guard_tests.rs:603` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/comparison_operand_guard_tests.rs:775` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/comparison_operand_guard_tests.rs:791` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/comparison_operand_guard_tests.rs:807` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/comparison_operand_guard_tests.rs:848` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/comparison_operand_guard_tests.rs:875` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/comparison_operand_guard_tests.rs:90` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/comparison_operand_guard_tests.rs:915` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/comparison_operand_guard_tests.rs:941` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/comparison_operand_guard_tests.rs:991` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/expr_error_sentinel_tests.rs:113` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/expr_error_sentinel_tests.rs:148` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/expr_error_sentinel_tests.rs:180` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/expr_error_sentinel_tests.rs:209` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/expr_error_sentinel_tests.rs:233` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/expr_error_sentinel_tests.rs:272` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/expr_error_sentinel_tests.rs:298` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/expr_error_sentinel_tests.rs:324` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/expr_error_sentinel_tests.rs:346` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/expr_error_sentinel_tests.rs:378` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/expr_error_sentinel_tests.rs:413` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/expr_error_sentinel_tests.rs:448` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/expr_error_sentinel_tests.rs:486` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/expr_error_sentinel_tests.rs:540` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/expr_error_sentinel_tests.rs:62` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/expr_error_sentinel_tests.rs:89` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/implies_type_check_tests.rs:22` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/mul_div_operand_guard_tests.rs:213` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/mul_div_static_runtime_parity.rs:386` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/polymorphic_zero_tests.rs:435` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/polymorphic_zero_tests.rs:500` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/polymorphic_zero_tests.rs:567` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/polymorphic_zero_tests.rs:653` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/polymorphic_zero_tests.rs:710` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/real_dimensionless_unification_tests.rs:113` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/self_keyword_tests.rs:1140` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/self_keyword_tests.rs:1187` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/self_keyword_tests.rs:1231` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/self_keyword_tests.rs:1262` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/self_keyword_tests.rs:1304` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/self_keyword_tests.rs:1334` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/self_keyword_tests.rs:1501` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/self_keyword_tests.rs:525` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/wrong_receiver_member_tests.rs:121` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/wrong_receiver_member_tests.rs:266` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/wrong_receiver_member_tests.rs:49` | `compile-error` |
| `crates/reify-compiler/tests/harness_type_checking/wrong_receiver_member_tests.rs:85` | `compile-error` |
| `crates/reify-compiler/tests/harness_units_materials/cost_robustness_tradeoff_lowering.rs:118` | `compile-error` |
| `crates/reify-compiler/tests/harness_units_materials/cost_robustness_tradeoff_lowering.rs:156` | `compile-error` |
| `crates/reify-compiler/tests/harness_units_materials/cost_robustness_tradeoff_lowering.rs:182` | `compile-error` |
| `crates/reify-compiler/tests/harness_units_materials/cost_robustness_tradeoff_lowering.rs:210` | `compile-error` |
| `crates/reify-compiler/tests/harness_units_materials/cost_robustness_tradeoff_lowering.rs:215` | `compile-error` |
| `crates/reify-compiler/tests/harness_units_materials/cost_robustness_tradeoff_lowering.rs:222` | `compile-error` |
| `crates/reify-compiler/tests/harness_units_materials/cost_robustness_tradeoff_lowering.rs:229` | `compile-error` |
| `crates/reify-compiler/tests/harness_units_materials/cost_robustness_tradeoff_lowering.rs:54` | `compile-error` |
| `crates/reify-compiler/tests/harness_units_materials/materials_fea_tests.rs:1054` | `compile-error` |
| `crates/reify-compiler/tests/harness_units_materials/materials_param_surface_tests.rs:367` | `compile-error` |
| `crates/reify-compiler/tests/harness_units_materials/materials_param_surface_tests.rs:468` | `compile-error` |
| `crates/reify-compiler/tests/harness_units_materials/money_force_diagnostic_tests.rs:113` | `compile-error` |
| `crates/reify-compiler/tests/harness_units_materials/money_force_diagnostic_tests.rs:143` | `compile-error` |
| `crates/reify-compiler/tests/harness_units_materials/money_force_diagnostic_tests.rs:176` | `compile-error` |
| `crates/reify-compiler/tests/harness_units_materials/money_force_diagnostic_tests.rs:60` | `compile-error` |
| `crates/reify-compiler/tests/param_binding_selector_coercion_tests.rs:110` | `compile-error` |
| `crates/reify-compiler/tests/param_binding_selector_coercion_tests.rs:139` | `compile-error` |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:112` | `compile-error` |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:232` | `compile-error` |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:27` | `compile-error` |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:366` | `compile-error` |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:407` | `compile-error` |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:440` | `compile-error` |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:468` | `compile-error` |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:506` | `compile-error` |
| `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:543` | `compile-error` |
| `crates/reify-compiler/tests/selector_composition_tests.rs:24` | `compile-error` |
| `crates/reify-compiler/tests/selector_composition_tests.rs:32` | `compile-error` |
| `crates/reify-compiler/tests/selector_composition_tests.rs:40` | `compile-error` |
| `crates/reify-compiler/tests/selector_composition_tests.rs:475` | `compile-error` |
| `crates/reify-compiler/tests/selector_composition_tests.rs:783` | `compile-error` |
| `crates/reify-compiler/tests/silent_defaults_tests.rs:16` | `compile-error` |
| `crates/reify-compiler/tests/silent_defaults_tests.rs:186` | `compile-error` |
| `crates/reify-compiler/tests/silent_defaults_tests.rs:211` | `compile-error` |
| `crates/reify-compiler/tests/silent_defaults_tests.rs:364` | `compile-error` |
| `crates/reify-compiler/tests/silent_defaults_tests.rs:715` | `compile-error` |
| `crates/reify-compiler/tests/silent_defaults_tests.rs:747` | `compile-error` |
| `crates/reify-compiler/tests/silent_defaults_tests.rs:777` | `compile-error` |
| `crates/reify-compiler/tests/termination_check_tests.rs:107` | `compile-error` |
| `crates/reify-compiler/tests/termination_check_tests.rs:173` | `compile-error` |
| `crates/reify-compiler/tests/termination_check_tests.rs:212` | `compile-error` |
| `crates/reify-compiler/tests/termination_check_tests.rs:278` | `compile-error` |
| `crates/reify-compiler/tests/termination_check_tests.rs:315` | `compile-error` |
| `crates/reify-compiler/tests/termination_check_tests.rs:375` | `compile-error` |
| `crates/reify-compiler/tests/termination_check_tests.rs:430` | `compile-error` |
| `crates/reify-compiler/tests/termination_check_tests.rs:519` | `compile-error` |
| `crates/reify-compiler/tests/termination_check_tests.rs:580` | `compile-error` |
| `crates/reify-compiler/tests/termination_check_tests.rs:622` | `compile-error` |
| `crates/reify-compiler/tests/termination_check_tests.rs:711` | `compile-error` |
| `crates/reify-compiler/tests/termination_check_tests.rs:760` | `compile-error` |
| `crates/reify-compiler/tests/termination_check_tests.rs:839` | `compile-error` |
| `crates/reify-compiler/tests/termination_check_tests.rs:864` | `compile-error` |
| `crates/reify-compiler/tests/unresolved_diagnostic_code_audit_tests.rs:127` | `compile-error` |
| `crates/reify-compiler/tests/unresolved_diagnostic_code_audit_tests.rs:158` | `compile-error` |
| `crates/reify-compiler/tests/unresolved_diagnostic_code_audit_tests.rs:191` | `compile-error` |
| `crates/reify-compiler/tests/unresolved_diagnostic_code_audit_tests.rs:222` | `compile-error` |
| `crates/reify-compiler/tests/unresolved_diagnostic_code_audit_tests.rs:255` | `compile-error` |
| `crates/reify-compiler/tests/unresolved_diagnostic_code_audit_tests.rs:289` | `compile-error` |
| `crates/reify-compiler/tests/unresolved_diagnostic_code_audit_tests.rs:325` | `compile-error` |
| `crates/reify-compiler/tests/unresolved_diagnostic_code_audit_tests.rs:93` | `compile-error` |
| `crates/reify-eval/tests/chained_comparison_eval.rs:305` | `compile-error` |
| `crates/reify-eval/tests/chamfer_e2e.rs:19` | `compile-error` |
| `crates/reify-eval/tests/compose_example_smoke.rs:134` | `compile-error` |
| `crates/reify-eval/tests/compute_dispatch_registry.rs:2131` | `compile-error` |
| `crates/reify-eval/tests/connect_eval.rs:138` | `compile-error` |
| `crates/reify-eval/tests/connect_eval.rs:732` | `compile-error` |
| `crates/reify-eval/tests/connect_eval.rs:943` | `compile-error` |
| `crates/reify-eval/tests/curve_constructors_e2e.rs:45` | `compile-error` |
| `crates/reify-eval/tests/curve_constructors_e2e.rs:548` | `compile-error` |
| `crates/reify-eval/tests/curve_constructors_e2e.rs:566` | `compile-error` |
| `crates/reify-eval/tests/curve_constructors_e2e.rs:584` | `compile-error` |
| `crates/reify-eval/tests/curve_constructors_e2e.rs:602` | `compile-error` |
| `crates/reify-eval/tests/e2e_meta.rs:337` | `compile-error` |
| `crates/reify-eval/tests/e2e_meta.rs:365` | `compile-error` |
| `crates/reify-eval/tests/extrude_e2e.rs:19` | `compile-error` |
| `crates/reify-eval/tests/extrude_e2e.rs:55` | `compile-error` |
| `crates/reify-eval/tests/extrude_infinite_e2e.rs:100` | `compile-error` |
| `crates/reify-eval/tests/extrude_infinite_e2e.rs:68` | `compile-error` |
| `crates/reify-eval/tests/fillet_e2e.rs:19` | `compile-error` |
| `crates/reify-eval/tests/harness_cache/unified_dag_geometry_executors.rs:911` | `compile-error` |
| `crates/reify-eval/tests/harness_geometry/geometry_length_args_units_e2e.rs:138` | `compile-error` |
| `crates/reify-eval/tests/harness_geometry/geometry_length_args_units_e2e.rs:156` | `compile-error` |
| `crates/reify-eval/tests/harness_geometry/geometry_length_args_units_e2e.rs:171` | `compile-error` |
| `crates/reify-eval/tests/harness_geometry/modify_sweep_length_units_e2e.rs:226` | `compile-error` |
| `crates/reify-eval/tests/harness_geometry/modify_sweep_length_units_e2e.rs:316` | `compile-error` |
| `crates/reify-eval/tests/harness_geometry/modify_sweep_length_units_e2e.rs:400` | `compile-error` |
| `crates/reify-eval/tests/harness_geometry/modify_sweep_length_units_e2e.rs:481` | `compile-error` |
| `crates/reify-eval/tests/harness_geometry/modify_sweep_length_units_e2e.rs:610` | `compile-error` |
| `crates/reify-eval/tests/harness_geometry/primitive_profile_length_units_e2e.rs:214` | `compile-error` |
| `crates/reify-eval/tests/harness_geometry/primitive_profile_length_units_e2e.rs:293` | `compile-error` |
| `crates/reify-eval/tests/harness_geometry/primitive_profile_length_units_e2e.rs:313` | `compile-error` |
| `crates/reify-eval/tests/harness_mechanism/mechanism_nondriving_joint_diag_e2e.rs:29` | `compile-error` |
| `crates/reify-eval/tests/harness_mechanism/mechanism_nondriving_joint_diag_e2e.rs:52` | `compile-error` |
| `crates/reify-eval/tests/harness_mechanism/mechanism_nondriving_joint_diag_e2e.rs:68` | `compile-error` |
| `crates/reify-eval/tests/harness_stress_scenarios/stress_error_messages.rs:125` | `compile-error` |
| `crates/reify-eval/tests/harness_stress_scenarios/stress_error_messages.rs:155` | `compile-error` |
| `crates/reify-eval/tests/harness_stress_scenarios/stress_error_messages.rs:87` | `compile-error` |
| `crates/reify-eval/tests/harness_stress_scenarios/stress_sweep_degenerate.rs:370` | `compile-error` |
| `crates/reify-eval/tests/keyed_identity_reelaboration.rs:117` | `compile-error` |
| `crates/reify-eval/tests/keyed_sub_eval.rs:78` | `compile-error` |
| `crates/reify-eval/tests/m5_integration.rs:792` | `compile-error` |
| `crates/reify-eval/tests/match_block_decls_e2e.rs:152` | `compile-error` |
| `crates/reify-eval/tests/mirror_circular_value_forms_e2e.rs:555` | `compile-error` |
| `crates/reify-eval/tests/mirror_circular_value_forms_e2e.rs:832` | `compile-error` |
| `crates/reify-eval/tests/money_acceptance_sweep_eval.rs:120` | `compile-error` |
| `crates/reify-eval/tests/nurbs_surface_e2e.rs:67` | `compile-error` |
| `crates/reify-eval/tests/pattern_spacing_units_e2e.rs:176` | `compile-error` |
| `crates/reify-eval/tests/pattern_spacing_units_e2e.rs:93` | `compile-error` |
| `crates/reify-eval/tests/region_resolution_boundary.rs:685` | `compile-error` |
| `crates/reify-eval/tests/region_resolution_boundary.rs:699` | `compile-error` |
| `crates/reify-eval/tests/structure_in_purpose_ambient_eval.rs:47` | `compile-error` |
| `crates/reify-eval/tests/sub_placement_surfacing.rs:745` | `compile-error` |
| `crates/reify-eval/tests/sub_placement_surfacing.rs:807` | `compile-error` |
| `crates/reify-eval/tests/type_hygiene_integration_gate.rs:310` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/ad_hoc_selector_tests.rs:114` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/ad_hoc_selector_tests.rs:58` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/annotation_tests.rs:579` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/annotation_tests.rs:630` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/annotation_tests.rs:673` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/annotation_tests.rs:712` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/forall_statement_tests.rs:171` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/forall_statement_tests.rs:222` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/forall_statement_tests.rs:30` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/forall_statement_tests.rs:343` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/forall_statement_tests.rs:394` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/forall_statement_tests.rs:446` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/forall_statement_tests.rs:99` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/guard_tests.rs:101` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/guard_tests.rs:126` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/guard_tests.rs:158` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/guard_tests.rs:197` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/guard_tests.rs:23` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/guard_tests.rs:239` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/guard_tests.rs:273` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/guard_tests.rs:46` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/guard_tests.rs:75` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/keyed_sub_member_block_parser_tests.rs:125` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/keyed_sub_member_block_parser_tests.rs:300` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/keyed_sub_member_block_parser_tests.rs:343` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/keyed_sub_member_block_parser_tests.rs:37` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/keyed_sub_member_block_parser_tests.rs:374` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/keyed_sub_member_block_parser_tests.rs:63` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/keyed_sub_member_block_parser_tests.rs:95` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/match_tests.rs:118` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/match_tests.rs:163` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/match_tests.rs:76` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/option_tests.rs:156` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax/purpose_tests.rs:224` | `compile-error` |
| `crates/reify-syntax/tests/harness_syntax_lowering/enum_named_field_lowering_tests.rs:157` | `compile-error` |

## Coverage and limitations

Of 701 tracked `.ri` members, **696 were surveyed** and **5 were not**. A further **78** were surveyed only PARTIALLY. Both are listed below rather than dropped: a bounded sweep that does not state what it skipped reads as full coverage and would under-size γ.

### Not surveyed (contributed no sites)

| file | reason |
|---|---|
| `crates/reify-cli/tests/fixtures/bracket_parse_error.ri` | `parse-error` |
| `docs/prds/v0_6/fixtures/indexed_sub_instantiation_surface.ri` | `parse-error` |
| `gui/test/fixtures/broken_syntax.ri` | `parse-error` |
| `tests/prd-gate/fixtures/adt_mirror_of_arm.ri` | `parse-error` |
| `tests/prd-gate/fixtures/arrow_type.ri` | `parse-error` |

### Partially surveyed (sites collected, but the file also failed to compile)

| file | reason |
|---|---|
| `crates/reify-cli/tests/fixtures/bracket_compile_error.ri` | `compile-error` |
| `crates/reify-cli/tests/fixtures/connect_direction_bare_mismatch.ri` | `compile-error` |
| `crates/reify-cli/tests/fixtures/connect_direction_dotted_mismatch.ri` | `compile-error` |
| `crates/reify-cli/tests/fixtures/keyed_missing_key.ri` | `compile-error` |
| `crates/reify-cli/tests/fixtures/objective_conflict.ri` | `compile-error` |
| `crates/reify-cli/tests/fixtures/result_prelude_pinned_mismatch.ri` | `compile-error` |
| `crates/reify-cli/tests/fixtures/variant_construct_missing_field.ri` | `compile-error` |
| `crates/reify-cli/tests/fixtures/variant_construct_payload_type.ri` | `compile-error` |
| `crates/reify-cli/tests/fixtures/variant_construct_unknown_field.ri` | `compile-error` |
| `crates/reify-compiler/tests/fixtures/coupling_motionvalue_mismatch.ri` | `compile-error` |
| `crates/reify-compiler/tests/fixtures/parametric_alias_def_site_reject.ri` | `compile-error` |
| `crates/reify-compiler/tests/fixtures/specialization_scope_forbidden.ri` | `compile-error` |
| `crates/reify-eval/tests/fixtures/selectors/bt1_wrong_kind_union.ri` | `compile-error` |
| `crates/reify-eval/tests/fixtures/selectors/bt6_kind_typed_param.ri` | `compile-error` |
| `crates/reify-eval/tests/fixtures/specialization_scope_forbidden.ri` | `compile-error` |
| `crates/reify-syntax/tests/fixtures/sub_placement_spec_example.ri` | `compile-error` |
| `docs/prds/v0_6/fixtures/hole_cutter_member_gap.ri` | `compile-error` |
| `docs/prds/v0_6/fixtures/member_chain_geom.ri` | `compile-error` |
| `docs/prds/v0_6/fixtures/member_fn_geometry.ri` | `compile-error` |
| `docs/prds/v0_6/fixtures/member_geom_alias.ri` | `compile-error` |
| `docs/prds/v0_6/fixtures/member_geom_let_instance.ri` | `compile-error` |
| `docs/prds/v0_6/fixtures/placement_b3_operand_type.ri` | `compile-error` |
| `docs/prds/v0_6/fixtures/placement_b5_world_frame.ri` | `compile-error` |
| `docs/prds/v0_6/fixtures/placement_relations_surfaces.ri` | `compile-error` |
| `docs/prds/v0_6/fixtures/shadow_oblig_redeclared.ri` | `compile-error` |
| `docs/prds/v0_6/fixtures/shadow_oblig_required.ri` | `compile-error` |
| `docs/prds/v0_6/fixtures/shadow_payload_binder.ri` | `compile-error` |
| `examples/auto/bearing_computed_default_unevaluated.ri` | `compile-error` |
| `examples/auto/bearing_constraint_select.ri` | `compile-error` |
| `examples/auto/bearing_unsat.ri` | `compile-error` |
| `examples/conditional_compilation/main.ri` | `compile-error` |
| `examples/module_visibility/consumer.ri` | `compile-error` |
| `examples/multi_aspect_objective_mixed.ri` | `compile-error` |
| `tests/prd-gate/fixtures/adt_relation_verbs.ri` | `compile-error` |
| `tests/prd-gate/fixtures/adv_beta_undef_arith_control.ri` | `compile-error` |
| `tests/prd-gate/fixtures/collection_sub_at_placement_rejected.ri` | `compile-error` |
| `tests/prd-gate/fixtures/compiler_type_hygiene_integration_gate.ri` | `compile-error` |
| `tests/prd-gate/fixtures/compiler_type_hygiene_mul_scale_guard_defeat.ri` | `compile-error` |
| `tests/prd-gate/fixtures/compiler_type_hygiene_mul_vec_silent_int.ri` | `compile-error` |
| `tests/prd-gate/fixtures/compiler_type_hygiene_trait_args_silent_accept.ri` | `compile-error` |
| `tests/prd-gate/fixtures/compose_middle_type_mismatch_rejected.ri` | `compile-error` |
| `tests/prd-gate/fixtures/compose_one_arg_rejected.ri` | `compile-error` |
| `tests/prd-gate/fixtures/curvature_rad_literal.ri` | `compile-error` |
| `tests/prd-gate/fixtures/dce_runtime_payload.ri` | `compile-error` |
| `tests/prd-gate/fixtures/dcr_fn_force_param_already_rejects.ri` | `compile-error` |
| `tests/prd-gate/fixtures/dwr_cantilever_qoi.ri` | `compile-error` |
| `tests/prd-gate/fixtures/dwr_qoi_without_adaptive.ri` | `compile-error` |
| `tests/prd-gate/fixtures/expected_type_pushdown_arg.ri` | `compile-error` |
| `tests/prd-gate/fixtures/expected_type_pushdown_let.ri` | `compile-error` |
| `tests/prd-gate/fixtures/forall_range_domain_rejected.ri` | `compile-error` |
| `tests/prd-gate/fixtures/indexed_sub_forall_range_baseline.ri` | `compile-error` |
| `tests/prd-gate/fixtures/indexed_sub_self_member_misrouted.ri` | `compile-error` |
| `tests/prd-gate/fixtures/indexed_sub_self_member_nogeom_unsupported.ri` | `compile-error` |
| `tests/prd-gate/fixtures/orient_axis_angle_member_parses.ri` | `compile-error` |
| `tests/prd-gate/fixtures/purpose_nested_structure.ri` | `compile-error` |
| `tests/prd-gate/fixtures/quantifier_expr_member_access_rejected.ri` | `compile-error` |
| `tests/prd-gate/fixtures/quantifier_expr_range_domain_rejected.ri` | `compile-error` |
| `tests/prd-gate/fixtures/raw_lambda_material_field_rejected.ri` | `compile-error` |
| `tests/prd-gate/fixtures/scalar_codomain_mismatch.ri` | `compile-error` |
| `tests/prd-gate/fixtures/self_collection_count_redirect_rejected.ri` | `compile-error` |
| `tests/prd-gate/fixtures/shear_angles_component_deg_compare_pre.ri` | `compile-error` |
| `tests/prd-gate/fixtures/shear_angles_vec3_wrongq_ctrl.ri` | `compile-error` |
| `tests/prd-gate/fixtures/solver_unification_tangent_silent_accept.ri` | `compile-error` |
| `tests/prd-gate/fixtures/stdlib_ns_mode_member.ri` | `compile-error` |
| `tests/prd-gate/fixtures/stdlib_ns_qualified_expr.ri` | `compile-error` |
| `tests/prd-gate/fixtures/stdlib_ns_qualified_type.ri` | `compile-error` |
| `tests/prd-gate/fixtures/typeparam_member_access.ri` | `compile-error` |
| `tree-sitter-reify/test/fixtures/ambient-default-1.ri` | `compile-error` |
| `tree-sitter-reify/test/fixtures/ambient-default-2.ri` | `compile-error` |
| `tree-sitter-reify/test/fixtures/dce-construction-expr.ri` | `compile-error` |
| `tree-sitter-reify/test/fixtures/gr-01-at-auto-relate.ri` | `compile-error` |
| `tree-sitter-reify/test/fixtures/gr-02-at-auto-where.ri` | `compile-error` |
| `tree-sitter-reify/test/fixtures/gr-05a-joint-with.ri` | `compile-error` |
| `tree-sitter-reify/test/fixtures/gr-05b-joint-with-rec.ri` | `compile-error` |
| `tree-sitter-reify/test/fixtures/mv-3-priv-sub-port.ri` | `compile-error` |
| `tree-sitter-reify/test/fixtures/mv-4-priv-let-constraint.ri` | `compile-error` |
| `tree-sitter-reify/test/fixtures/trait_assoc_type_bind.ri` | `compile-error` |
| `tree-sitter-reify/test/fixtures/trait_assoc_type_qual.ri` | `compile-error` |

### Named limitations

1. **Inline Reify snippets are reached as RAW-STRING LITERALS, and only as those.**
The *Inline Rust fixtures* section above sweeps every tracked `.rs` test host
for raw-string literals (`r"…"`, `r#"…"#`) whose text reads as Reify
declaration grammar, and compiles each through the same pipeline as a tracked
`.ri`. That reached **3198 inline member(s)**, of which **71** were
`format!` template(s) — listed above under their own coverage reason rather
than dropped, because a template's `{…}` holes are not Reify syntax and a
parse failure on one would say nothing about conformance.
What a raw-string walker does **not** reach, each named by the construct to
grep for: Reify text carried in an ORDINARY `"…"` string literal (including
the backslash-continued multi-line form); text assembled by `concat!`; and text
built at run time by a `String` helper (`push_str`, `join`). Those are
unreached BY CONSTRUCTION, not by oversight — recovering them needs
const-evaluation or execution where this needs only a lexer — so a site in one
of those shapes is absent from the section above rather than reported clean.
`include_str!` and `read_to_string` goldens, by contrast, need no machinery at
all: their target `.ri` files are tracked, so the FIRST half already
enumerated them.
2. **`compile_with_stdlib` is the SINGLE-FILE path.** `reify check` instead uses
`module_dag::compile_entry_with_stdlib_cfg_checked`, which follows `#cfg`-gated
user imports and runs `SimpleConstraintChecker`. Multi-module corpus members
(the `examples/module_visibility/consumer.ri` class) therefore cannot resolve
standalone and appear above under *not surveyed* or *partially surveyed* with
their reason, rather than being silently dropped.
3. **`def` resolution uses ONE GLOBAL namespace, not per-file module scope.** The
`owner` grouping cross-checks a recovered name against every `structure def`
declared anywhere in the corpus plus the stdlib — it does NOT ask whether that
declaration is visible from the file the row sits in. Two consequences, and
only one of them is safe:
**(a)** a row lands in *non-FEA — γ per-case judgment* whenever SOME corpus
file declares that name, even if the row's own file cannot see it. That is
over-inclusion in the TOUCHABLE direction, so **before treating a `non-FEA` row
as actionable, confirm the `def` is declared in that row's own file or one of
its imports.** The `message` and `severity` columns usually settle it in one
read.
**(b)** symmetrically, a non-FEA file that happened to declare a name the FEA
stdlib also declares would pull its sites into the do-not-touch partition. That
direction over-defers rather than over-touches, so it costs γ sizing accuracy,
never a wrong edit.
Per-member scoping (each file's own declarations plus its imports) would remove
the approximation, at the cost of resolving the import graph for every member —
more machinery than a one-shot snapshot warrants, so the approximation is stated
here instead of hidden.

## How to regenerate

```bash
env cargo test -p reify-compiler --test harness_compilation_surface -- --ignored --exact ctor_conformance_corpus_survey::generate_ctor_conformance_corpus_survey
```

The generator is `#[ignore]`d: it compiles the whole tracked corpus, which is
~2.5× the `examples/` walk already documented as the most expensive thing that
test binary does, and paying that on every merge gate would fight
`docs/prds/merge-gate-compile-cost.md`. Everything the generator *decides* —
enumeration, span→line, def/field/type extraction, D9 classification and this
rendering — is unit-tested on every gate run against synthetic inputs, plus one
cheap three-file end-to-end sweep, so the pipeline cannot bit-rot between runs.

Set `REIFY_CTOR_SURVEY_OUT` to write elsewhere (e.g. to diff a fresh run against
the committed copy without dirtying the tree).

> The `env` prefix on the command above bypasses reify's PreToolUse hook, which
> condenses `cargo test` output. It is harmless here — the generator writes the
> file rather than being scraped from stdout — but without it a reader of the run
> log sees only a `PASS: N | FAIL: M` summary and may think the sweep did nothing.
