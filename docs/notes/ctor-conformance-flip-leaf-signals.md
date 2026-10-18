# Before-image leaf signals after the ctor-conformance severity flip (#5306)

Point-in-time routing record, written by task #7372 on 2026-09-25 against main
12a9da343d. The prd-gate before-image fixtures under `tests/prd-gate/fixtures/`
each keep their OWN measurements in their header: the decompose record, plus a
POST-δ RE-MEASUREMENT block with the current exit codes and diagnostic set.
They cite this note for the one thing a measurement cannot say, which is how
each owning leaf reads its signal now that the file's exit code no longer
reflects it. Everything below is a plan or a prediction, not a measurement.
Where a leaf's task record disagrees with this note, the task record wins.

"δ" here is PRD `docs/prds/v0_6/dimensioned-construction-strictness.md`'s leaf
δ (#5306). It is NOT the joints leaf δ of `dimension-checked-readers.md` §9.

## What changed

#5306 flipped `CTOR_FIELD_CONFORMANCE_SEVERITY`
(`crates/reify-compiler/src/conformance/mod.rs`) from Warning to Error. A
struct-ctor call whose argument dimension disagrees with the declared field
type is now a compile-time `ArgTypeMismatch` Error. `reify check` and
`reify eval` both exit 1, and eval never reaches a native reader.

The before-images for `docs/prds/v0_6/dimension-checked-readers.md` built
their wrong-dimension values through exactly such ctor calls. Five rules
follow for every leaf that owns one of them:

1. **The fixture's exit 1 is δ's, never the leaf's.** A leaf that reads its
   signal from that exit code reads δ.
2. **Dimensioning the call site is not a route.** It turns the file into its
   positive twin and erases the defect the file pins.
3. **Reader rejections are read below the compiler.** A Rust test hands the
   reader a hand-built `Value` carrying the wrong dimension, and asserts
   `DiagnosticCode::DimensionedArgRejected` identity (INV-SF-6). That is the
   code the PRD's §6 decision 1 reconciliation (ruling esc-5791-3) names for
   reader and field rejections. `ArgDimensionMismatch` was never minted.
4. **A positive-half floor is read from a file WITHOUT the wrong binding.**
   The whole file fails to compile, so a correct binding sharing it with a
   wrong one is not evaluated either.
5. **The `.ri`-level rejection is δ's now.** Each reader gate below is a second
   line behind it. OPEN QUESTION, not a known path: a `.ri`-level reader
   signal would need a construction path that the ctor-conformance walk does
   not govern.

The machine pin for "this site still fails at δ" is the gate-resident
`ctor_conformance_corpus_residual_entries_are_all_live`
(`crates/reify-compiler/tests/harness_ctor_conformance_survey/disposition.rs`).
Each owning leaf deletes its own `CTOR_CONFORMANCE_CORPUS_RESIDUAL` entries in
its own diff.

## Per leaf

### β (#6941): rows B4 and B5

- **B4, `dcr_material_dimension_silent.ri`.** The flexure reader
  (`material_field_si`) is unreachable from the file (rule 1), and the 200GPa
  spelling is the positive twin `dcr_material_dimension_correct.ri` (rule 2).
  β feeds the reader a hand-built `Material` whose `youngs_modulus` is a
  LENGTH Scalar (rule 3).
- **B5, `dcr_yield_stress_dimension_silent.ri`.** Same route as B4, with
  `yield_stress` set to `Some(<LENGTH Scalar>)`. β's floor, that
  `some(310MPa)` keeps `prb_validity_range` at ±0.08726646259971647 rad, is
  read from a file without the `wrong` binding (rule 4).

### ε, η (#6922) and ζ (#6941): the reader ctor sites

- **`dcr_reader_ctor_dimension_silent.ri`.** Its per-binding `ArgTypeMismatch`
  set belongs to δ. Nothing in it can hand a wrong value to `cell_f64` /
  `mass_properties_from_value` (ε), `read_scalar_si` (ζ), or `field_scalar` /
  `opt_f64` (η). Each leaf feeds its own reader a hand-built struct value
  (rule 3).
- **ζ, `dcr_shaper_frequency_dimension_silent.ri`.** The consuming path this
  fixture was amended to exercise (`input_shape` → `build_train_for_shaper` →
  `read_scalar_si`) is unreachable while `shaper_rads` is in the file. ζ feeds
  the consolidated reader a hand-built `ZVShaper` carrying rad·s^-1 (rule 3).
  ζ's floor, that the 50Hz half stays byte-identical, is evaluated from a file
  without `shaper_rads` (rule 4).

### γ1 (#6922): row B1, `dcr_solver_load_dropped_dimensioned.ri`

The call site is ALREADY units-correct. What fails is the field's `Real`
declaration, so rule 2 does not arise. The B1 negative-assertion mandate
still applies: the force must be APPLIED, not merely diagnosed. There are
two routes:

- **Below the compiler.** A Rust test feeds the shared load reader / solve
  path a `PointLoad` Value whose force is a FORCE Scalar of 1000 N, and
  compares the result against the bare control.
- **End to end through `.ri`, once γ2 has retyped `PointLoad.force`.** The file
  then compiles clean, and `max_von_mises` must equal the bare control's
  5139325.408614099 Pa with non-zero iterations. This route inverts the
  current #6941 → #6922 dependency edge.

### γ2 (#6941): the four `fea_multi_case.ri` load fields

Retyping the fields makes γ2's signal compile-time and direct. PREDICTION,
probe-backed: a scratch structure declaring `param force : Force = 0N`, given
`force: 1000.0`, measured check 1 / eval 1 with "argument 'force' has type
'Real' but param 'force' requires type 'Scalar[m·kg·s^-2]'". Consequences:

- **`dcr_load_ctor_dimension_silent.ri`.** Its two current Errors
  (`dimensioned_point`, `traction`) vanish. The three bare spellings
  (`bare_point`, `bare_pressure`, `body_force`) each raise ctor-conformance
  `ArgTypeMismatch` instead, so the file STILL exits 1 after γ2, on the
  opposite bindings. γ2 reads its signal from the per-binding diagnostic set,
  never from the exit code.
- **`dcr_solver_load_dropped_bare.ri`.** The control's own `force: 1000.0`
  becomes a compile-time Error, so γ2 migrates or retires the control in its
  own diff. Once migrated, it is identical to the dimensioned twin.

### γ3 (#5802): rows B2 and B3

- **B2 (bare spelling → `DimensionedArgRejected`).** Pre-empted from `.ri`:
  after γ2, a bare load never reaches `extract_loads`. γ3 observes its reader
  narrowing below the compiler (rule 3).
- **B3 (`FeaLoadKindUnsupported`).** Needs a solver call, which
  `dcr_load_ctor_dimension_silent.ri` never makes.

### κ and θ (#6942): `dcr_langsurface_crossdim_silent.ri`

Unaffected by δ. The file has no struct-ctor call site, so its exit 0 → 1
flip is still κ's and θ's own `.ri` signal. Per offending call, the code is
`DimensionedArgRejected` (rule 3's ruling), while the file's listed controls
stay unchanged.

### α (#6179, `angle-dimension-completion.md`): `curvature_rad_literal.ri`

The file was already red at decompose, on α's own `ParamDefaultTypeMismatch`.
δ added a second Error keyed on the same mismatch. PREDICTION: α's
re-dimension of CURVATURE clears both, so α's check 1 → 0 flip survives δ as
α's signal. α asserts exit 0 with BOTH codes gone, not merely the loss of
`ParamDefaultTypeMismatch`.
