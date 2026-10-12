# Trampoline decode and result build: one decoder per stdlib input structure, one builder per result structure

**Milestone:** v0_6 · **Status:** active (decomposed 2026-10-12; leaf ids in §10 and the sidecar) · **Date:** 2026-10-12 · **Approach:** B + H (contract + two-way boundary tests; FEA and ComputeNode dispatch are overlay-listed load-bearing seams, blast radius is 6 crates)

**Program:** `plans/review-all-program-reify-20261012.md`, stream **S15** (wave 1, agent, size M, risk medium). The stream's remedy, deletable list and seams were ruled in the "Uncontested block" decision of that document's §Resolved decisions. This PRD does not reopen them.

**Code anchors** were verified against main `8e6ffcac9d` (2026-10-12). Every path this PRD builds on is byte-identical between the review's pinned tree `a030ee7528` and `8e6ffcac9d` (`git diff a030ee7528..8e6ffcac9d` over each path is empty). Main moves fast, and three in-flight tasks rewrite several of these files (see §8). Cite by symbol, and re-locate each symbol at implementation time.

**Normative substrate:** `docs/legibility/design-invariants.md`. This PRD gives **INV-PD-2** (`result-fields-populated-or-owned`) its runtime half. It provides the decoder sites that **INV-PD-1**'s (`declared-param-reaches-kernel`) PDROP declaration attaches to, and complies with **INV-SF-6** (`diagnostics-carry-codes`), **INV-SF-2** and the umbrella `nothing-vacuous-and-unowned`. Quality heuristics are cited by name from `/home/leo/src/dark-factory/docs/code-quality.md`.

---

## 1. Goal

Every compute trampoline reads the shared stdlib input structures (`ElasticMaterial`, `ElasticOptions` / `ModalOptions` / `BucklingOptions`, the load kinds, `Support`) through **one typed decoder per structure**. Each decoder is built on `reify_ir::arg_acceptance` and returns **one error type**, and that error becomes `ComputeOutcome::Failed` at **one point**. Every Rust producer writes the result structures it mints (`ElasticResult`, `ModalResult`, `Mode`, `MassProperties`) through **one builder per structure**, and a test pins each builder's emitted key set to the structure's declared `field_layout`.

After this PRD, a user or agent can observe the following:

1. **One failure policy.** A malformed or wrong-dimension `ElasticMaterial` reaching any of the four trampolines that read it (`solve_elastic_static`, `solve_buckling`, `modal_analysis`, `as_printed_material`) yields the same coded `ComputeOutcome::Failed`, naming `ElasticMaterial.<field>`. Today the same input gets a typed error in one, a panic in another, a silent `0.0` in a third and an `Option` in the fourth.
2. **Defaults have one home.** No Rust literal restates a declared `.ri` default. Editing `param max_iter : Int = 1000` in `solver_elastic.ri` changes the solve. Today three homes disagree (fk-cee09de8be7a).
3. **Written equals declared.** A builder that writes a field the `.ri` does not declare fails a test that names the field. Today `ElasticResult.warm_started` and `ModalResult.topology` are written undeclared, and nothing catches it.
4. **Typed diagnostics.** The 24 modal message-prefix mnemonics become `DiagnosticCode` variants, and `I_MechanismModalNModesTruncated` is emitted at Info severity.
5. **Loud stress kernels.** `von_mises([[1,2,0],[0,0,0],[0,0,0]])` makes `reify eval` exit 1 with a coded diagnostic. Today it panics in a debug build and silently returns the upper-triangle answer in release.
6. **One grid layout.** `SampledField` states its Regular3D axis order in one accessor. The transposed slab producer feeding `shell-extract` is fixed. Every shell-extract entry point enforces the same grid invariant set.

**Consumers (G1).** Each consumer below plugs into an existing seam:

- The trampolines at the ComputeNode-dispatch seam (`docs/prds/v0_3/engine-integration-norm.md` §3.4).
- The persistence bridge (`compute_persist`, `elastic_result_from_value`).
- **S5**, which moves the compute targets into the new analysis crate and carries these decoders and builders with them.
- **S16**, whose typed request and single result builder are built on §4's decoders and builders.
- The PVAC (#7099–#7102) and PDROP (#7079–#7085) chains, which attach their declarations to the single decoder and builder sites.
- `.ri` authors, who read coded diagnostics instead of plausible wrong numbers.

## 2. Background (verified on main 2026-10-12)

The finding records in `plans/review-all-reify-20261010.json` carry the full measurements. Only the load-bearing facts are repeated here.

- **Four `ElasticMaterial` reader dialects** (fk-34754bed8353):
  - `elastic_static::isotropic_from_data` returns a `Result`, but its scalar read is dimension-blind.
  - `buckling::extract_material` panics.
  - `modal_ops::extract_isotropic_material` turns a missing field into `0.0`.
  - `as_printed_material::read_base_elastic` returns an `Option`.

  `modal_ops::read_scalar_si` accepts a Scalar of any dimension and maps every non-numeric value to `0.0`. It has 9 call lines and backs 19 input reads, including the beam geometry, E and ν, the loss factor, the reference direction and mode-shape components. `modal_ops.rs` holds 19 silent-zero fallbacks in production.
- **Ctor-built instances always carry every declared field.** The compiler lowers a `structure def` call to `CompiledExprKind::StructureInstanceCtor` with `defaults: (param_name, default_expr)` for every param not supplied (`reify_ir::expr`). Every `options : XOptions = XOptions()` parameter and every `LoadCase.options` fallback (`multi_case`: `Option(Some(x))` gives a per-case override, anything else gives the shared instance) therefore reaches a trampoline fully populated. Every Rust fallback literal is reachable only from hand-built `Value`s. A `git grep` over production code finds none: every `type_name: "ElasticOptions"` construction sits in a `#[cfg(test)]` module. This is the fact that makes §6 D1 safe.
- **ElasticResult** (fk-b5dc0cd8ec92) is spelled at 3 writer sites (the shell arm, the tet arm, `value_from_elastic_result`) and 1 reader site (`elastic_result_from_value`). All three writers emit `warm_started`, but `structure def ElasticResult` (`solver_elastic.ri`) declares 15 params and no `warm_started`. The only reader is `crates/reify-eval/tests/harness_fea_solver_e2e/fea_cold_start_heuristic_e2e.rs`.
- **ModalResult / Mode** (fk-340a2cbd265b) have 3 builders and 2 builders respectively. All three ModalResult builders write the undeclared `topology` field, whose owner is #7097 (pending). Two docstrings claim "6 fields".
- **MassProperties** (fk-dc44c395ee01) has two minters: stdlib `dynamics::eval::make_mass_properties`, and reify-eval `dynamics_ops::assemble_mass_properties` + `inertia_value`. It has two inertia parsers: stdlib `inertia_3x3_from_value` and `dynamics_psd::inertia_3x3_from_value`. reify-eval already normal-depends on reify-stdlib.
- **Registry-free identity** (fk-a28b1b57ed64) has 5 production `const REGISTRY_FREE_TYPE_ID` copies (appearance, as_printed_material, dynamics_ops, analysis, dynamics/eval) and 155 literal `StructureTypeId(u32::MAX)` uses. Rust producers in `shell_extract_compute.rs` (8 sites) and `tensegrity.rs` (10 sites) mint with `StructureTypeId(0)`.
  - **G6 premise correction:** the 0-tag is wider than those 18 sites. The compiler's ctor lowering bakes `StructureTypeId(0)` into *every* user ctor instance by design. Identity is `(type_name, version)`, and esc-3540-177 RULING 2 says "any site needing registry side-table access re-stamps the real id by name".
  - Two registry reads do not re-stamp: `value_type_kind_matches`'s `Type::TraitObject` arm (`reify_eval` crate root) and `validate_param_override`'s debug check (`engine_admin`). Each calls `registry.meta(data.type_id)`, which judges every ctor-built instance against the bounds of whichever structure `StructureRegistry::intern` assigned id 0. Leaf δ closes the class at those two reads, not only at the 18 producer sites.
- **`Value::as_f64`** (fk-d30cf96fc557, filed as **#8559**) has 8 equivalent private copies and 2 drifted ones. Tasks #6922 and #6941 retire 5 of them (the three `cell_f64` copies and the two trajectory `read_scalar_si`s). The residue is `appearance::color_cell_f64`, `bom_report::value_as_f64`, gui `engine.rs::scalar_to_f64`, `tensegrity_crack::scalar_f64` (drops the Int arm) and `modal_ops::read_scalar_si` (maps non-numeric values to `0.0`).
- **Modal diagnostics** (fk-9082d3765668) use 24 distinct `[EWI]_` prefixes, and only 3 carry a `.with_code(DiagnosticCode::…)`. `I_MechanismModalNModesTruncated` is emitted via `Diagnostic::warning`.
- **Stress kernels** (fk-da538013331f): the symmetry tolerance closure is copied 3 times in `analysis.rs`. There is no check at the `Value` boundary, so an asymmetric `.ri` matrix panics in debug.
- **Supports** (fk-f33ff551c66d, weakened): the skeptic's narrowed reading binds here. The real SPOT problem is that `modal_ops::DeclaredSupport` decodes `PinnedSupport` by `type_name` string, and the shell route has no support decode. The elastic all-DOF clamp on tets is documented-correct. `reify_solver_elastic::shell_boundary::SupportKind::Pinned` is never constructed in production. The only `build_support_bcs` call, in the solver's `shell_solve`, passes `Fixed` on a synthesized `x == 0` root edge, so a user `PinnedSupport` on a shell-routed body is dropped silently.
- **`PressureSpec.direction`** (fk-2ac688334abf, filed as **#8524**) is parsed and never read. An unresolvable face is reported by `tracing::warn` only.
- **`shell_extract_compute_fn`** (fk-c02d5d2d8515) builds all 13 `ComputeOutcome::Failed` returns inline, and the phase-error → `DiagnosticCode` mapping has no home.
- **Grid layout** (fk-4ac205f76343):
  - `SampledField`'s doc says only "Flat row-major data".
  - `medial::sample_at_index`, `reify-expr` `interp::index_3d` and `sampled_fd` each spell the index by hand.
  - `compute_targets::shell_solve::build_slab_sdf` loops z-outermost over `axis_grids [x, y, z]`, which is transposed relative to every reader.
  - `medial` and `walk_direction` import each other.
  - Six private `slab_sdf_3d` / `regular3d_field` fixtures exist.
- **Grid validation** (fk-90433ba69658): `grid_validation::validate_regular3d` checks kind, axis lengths and non-empty axes. Data length and axis geometry are checked only in `compute_medial_mask`, so `extract_mid_surface` panics with an index out of bounds on a short buffer. `MidSurfaceError` has no variant for that case.
- **Grid equality** (fk-9bc22225093b): `fea::grids_equal` duplicates `SampledField::grid_metadata_eq` with different coverage, and its doc cites a dead path. `fea.rs` production holds 6 metadata-copy `SampledField` literals.
- **h14 partitions** recorded `stays_whole: false`: `crates/reify-stdlib/src/fea.rs` has 8 partitions (6,617 lines) and `crates/reify-stdlib/src/dynamics/eval.rs` has 7 (4,835 lines). Their symbol groups and symptoms are in the record's `h14_measurements`, and leaves ξ and ο follow them.

## 3. Sketch of approach

Three layers, in dependency order.

**Layer 1: decoders.** A new reify-eval module `compute_targets::stdlib_inputs` holds one typed decoder per shared stdlib input structure, plus the single error type and the single `Failed` mapping. Task #6922 creates `fea_inputs`, `fea_loads` and `fea_value_shape` (shared readers for `solve_elastic_static` and `solve_buckling`), and `stdlib_inputs` generalizes them: `FeaValueShapeError` becomes `InputDecodeError`. Every decoder takes its dimension policy from `reify_ir::arg_acceptance` (`accept_arg` / `accept_field` with the existing spec constructors) and holds no default literal (§6 D1). Trampolines call decoders and use `?`. The only conversion to `ComputeOutcome::Failed` is `InputDecodeError::into_failed`.

**Layer 2: builders.** Each result structure gets one builder that takes a typed record and returns the `Value`, and the cache bridge decodes through the inverse of the same pair. One `reify-ir` constructor, `StructureInstanceData::registry_free`, mints every Rust-side instance. Each builder ships a parity test asserting that its emitted key set equals the registry `field_layout` for the structure, minus an explicit undeclared-write list whose entries name a live owning task.

**Layer 3: leaf primitives and partitions.** These are the primitives the decoders and builders surfaced:

- `SampledField` gains `flat_index_3d` / `at_3d` / `from_fn_3d` / `same_grid` / `with_data`.
- shell-extract gets a `grid` leaf module and full grid validation.
- The stress builtins get one symmetry check at the `Value` boundary.
- The two `stays_whole: false` stdlib files are partitioned along their measured groups.

Each extracted primitive ships with an adoption-ratchet row (§4 C6, INV candidate 5).

## 4. Contract (H)

### C1: the decoder

```rust
// crates/reify-eval/src/compute_targets/stdlib_inputs/  (mod.rs + one file per structure family)
pub(crate) trait DecodeStdlibInput: Sized {
    const STRUCTURE: &'static str;            // the stdlib structure_def name
    fn decode(value: &Value) -> Result<Self, InputDecodeError>;
}
```

Invariants:

1. **One decoder per structure.** No production code outside `stdlib_inputs` reads a declared param of a decoded structure by string (`fields.get("<param>")`). The C6 ratchet enforces this per structure.
2. **No default literals (D1).** A declared non-`Option` field that is absent is `InputDecodeError` with failure `Missing`. An `Option<T>` field that is absent or `Option(None)` decodes to `None`. A decoder never substitutes a value for a declared param, because the declared default already arrived through the ctor.
3. **Dimension policy from `arg_acceptance` only.** Each dimensioned position names its spec constructor (`pressure_spec`, `length_spec`, `time_spec`, `dimensionless_spec`, …). Never write a hand-spelled `DimensionVector` or a `Scalar { si_value, .. }` match (`arg_acceptance` §7 I1: "pass the spec down; do not restate it").
4. **Enum- and string-typed params decode into Rust enums.** Examples: `ShellForce`, `ElementOrder`, `PressureDirection`, `SupportKind`. An unknown variant or string is `InputDecodeError` with failure `UnknownVariant`, never a fallthrough default.
5. **Ignored params stay owned by PDROP.** A decoder decodes exactly the params its trampoline reads today, so β changes no honored set. Declared params it does not read keep their `trampoline-param-drop-closure.md` §7 owners. #7079 then attaches its C1 declaration to the decoder itself (that PRD's §12 OQ2: the declaration lives beside the extraction code), and #7080/#7084 honor their params through the decoder's typed fields (D2).

### C2: the error type and the single `Failed` point

```rust
pub(crate) struct InputDecodeError { pub structure: &'static str, pub field: Option<&'static str>, pub failure: DecodeFailure }
pub(crate) enum DecodeFailure {
    NotAStructure { got: String },          // value is not a StructureInstance of `structure`
    Missing,                                 // declared non-Option field absent
    WrongShape { expected: &'static str, got: String },
    Rejected(reify_ir::arg_acceptance::ArgRejection),   // wrong dimension
    UnknownVariant { got: String },
    UnsupportedKind { kind: String },        // e.g. a load type_name with no solver support
}
impl InputDecodeError { pub(crate) fn into_failed(self, target: &str) -> ComputeOutcome; }
```

`into_failed` is the only place a decode error becomes `ComputeOutcome::Failed`. It emits exactly one `Severity::Error` diagnostic with these properties:

- `DiagnosticCode::DimensionedArgRejected` for `Rejected`. This reuses the existing code, per esc-5791-3: "one code per rejection reason".
- `DiagnosticCode::FeaLoadKindUnsupported` for `UnsupportedKind`, and `DiagnosticCode::ComputeInputMalformed` for every other `DecodeFailure` variant. This is a new code. Its doc-block names the origin, the firing rule and that it is unreachable from ctor-built values.
- A message formatted as `ArgRejection::message(target, "<Structure>.<field>")`, using the dotted-path convention `accept_field` already documents. There is no second formatter.

`UnsupportedKind` maps to the existing `DiagnosticCode::FeaLoadKindUnsupported`, which #5791 (dimension-checked-readers α, done) already minted. This applies one code per rejection reason. #5802 (γ3) owns which load kinds are unsupported on which solver path and owns the citations to the live owners in the message (#5800, #7081). It edits the `LoadSpec` decoder, not per-trampoline readers. (Corrected at decompose 2026-10-12: the author-time text predated finding the code on main.)

### C3: the builder

```rust
// one per result structure, beside its producer family
pub(crate) fn build_elastic_result(r: &ElasticResultParts) -> Value;      // + elastic_result_from_value(&Value) -> Option<ElasticResultParts>
pub(crate) fn build_modal_result(r: ModalResultParts) -> Value;          // + build_mode(ModeParts) -> Value, and their typed decode used by transient/displacement_at
pub fn MassProperties::to_value(&self) -> Value;                          // reify-stdlib dynamics::mass_props; + from_value
```

Invariants:

1. A builder is the only production site that writes the structure's `type_name` literal. C6 enforces this per structure.
2. Every builder constructs through `StructureInstanceData::registry_free` (C5).
3. The emitted key set equals the declared field set, plus an **undeclared-write list** in which each entry is a `(field, #NNNN)` pair naming a live task. The C4 parity test checks it. Today the list has one entry: `ModalResult.topology → #7097`.
4. A field the producer cannot compute is the honest `Undef`, as INV-PD-2 C2′ already requires. It is never a well-formed fake.

### C4: the parity test (INV-PD-2's runtime half)

For each builder, a test builds a representative value and compares its key set against `Engine::new(..).structure_registry().meta(id_for("<Structure>")).field_layout`. `populate_structure_registry` already sources that registry from the stdlib templates. The representative value comes from the production entry, or from the builder with production-shaped parts. The test fails, naming the field, in three cases: a written-but-undeclared field that is not on the undeclared-write list; a declared field the builder never writes; or a list entry whose field is now declared (the entry is stale). Its fixture self-test injects `warm_started` into a builder whose `.ri` lacks it and asserts the failure names `warm_started`.

**Scope boundary with PVAC.** This test is the per-builder runtime key-set check. It is **not** `reify-audit --pattern PVAC`, the static disposition gate that `result-field-vacuity-closure.md` owns (#7102). PVAC's C1′ dispositions (`populated` / `degraded` / `allowlisted`) attach to the C3 builders, which are now the single producer site for each structure. The program doc's wording for this stream ("the chartered PVAC detector") is reconciled in §13 item 1.

### C5: the registry-free constructor and name re-stamp

```rust
impl StructureInstanceData { pub fn registry_free(type_name: impl Into<String>, fields: PersistentMap<String, Value>) -> Self; }
impl StructureTypeId { pub const REGISTRY_FREE: StructureTypeId = StructureTypeId(u32::MAX); }
```

- Rust producers construct only through `registry_free`. The five private consts and the `StructureTypeId(0)` placeholders minted by Rust producers are deleted.
- The compiler's ctor placeholder `StructureTypeId(0)` stays as ruled (esc-3540-173/-177). Its consumers are fixed instead: every registry side-table read resolves `registry.id_for(&data.type_name)` (RULING 2) rather than trusting `data.type_id`. The two reads that do not do this today are `value_type_kind_matches`'s TraitObject arm and the `validate_param_override` debug check.

### C6: the adoption ratchet (INV candidate 5)

One Python stdlib-`unittest` infra member, `tests/infra/test_trampoline_decode_ratchet.py`, with its thin `.sh` wrapper and its `run-all-classification.manifest` row. It holds one baseline row per extracted primitive and counts the superseded inline pattern over production code (each file cut at its first top-level `#[cfg(test)]`). It reds when a count rises, and also reds when a count falls without a baseline edit (the stale-baseline shape of the PTODO ratchet). Each leaf that extracts a primitive adds its row in its own diff. The member asserts no wall-clock bound. Rows:

| row | inline pattern counted | home |
|---|---|---|
| `decoded-param-read` | per decoded structure: `fields.get("<param>")` for each declared param of that structure, outside `stdlib_inputs` (reify-eval production) | α, β, γ |
| `read-scalar-si` | `fn read_scalar_si` definitions + calls in `crates/reify-eval/src` | α (target 0) |
| `modal-silent-zero` | `unwrap_or(0.0)` / `=> 0.0` / `[0.0; 3]` in `modal_ops.rs` production | α (baseline = audited residual) |
| `inline-failed` | `ComputeOutcome::Failed {` literal constructions in `stdlib_inputs`-consuming trampolines + `shell_extract_compute.rs` | α, λ |
| `result-type-literal` | `type_name: "<Structure>"` writes outside the builder, per built structure | ε, ζ, η |
| `registry-free-literal` | `StructureTypeId(u32::MAX)` and `StructureTypeId(0)` in Rust producer code outside `reify-ir` and the compiler/expr ctor path | δ (target 0) |
| `value-f64-copy` | `fn` definitions taking `&Value` and returning `Option<f64>` with the Int/Real/Scalar arms | θ |
| `symmetry-closure` | the `1e-10 * (1.0 + a.abs().max(b.abs()))` closure outside `is_symmetric_3x3` | κ (target 0) |
| `grid-index` | hand-spelled `i * nj * nk + j * nk + k` / `index_3d` flattening outside `SampledField::flat_index_3d` | μ |
| `grid-metadata-copy` | production `SampledField { … oob_emitted: AtomicBool::new(false) }` literals that copy another field's grid | ξ |

## 5. Boundary-test sketch (H)

Facing both sides of the trampoline seam. "Hand-built" means a `Value` constructed in Rust below the compiler. This is the esc-7372-3 signal shape, since #5306 made ctor-field conformance a compile-time Error and a malformed `.ri` spelling now exits at compile before any reader runs.

| # | Scenario | Preconditions | Asserts |
|---|---|---|---|
| T1 | One policy for a malformed material | hand-built `ElasticMaterial` missing `poisson_ratio`, fed to the four trampolines (elastic, buckling, modal free-vibration, as-printed) | each returns `ComputeOutcome::Failed` with one Error, `DiagnosticCode::ComputeInputMalformed`, message naming `ElasticMaterial.poisson_ratio`. Today: typed error / panic / `ν = 0.0` silently / `None` |
| T2 | One policy for a wrong-dimension material | hand-built `youngs_modulus` = LENGTH Scalar | each of the four: `Failed` with `DimensionedArgRejected` naming `ElasticMaterial.youngs_modulus` |
| T3 | Ctor-built inputs are untouched | every tracked `examples/*.ri` and `tests/prd-gate/fixtures/*.ri` that reaches these trampolines | identical printed results before and after (the regression floor). Drifted literals are compared against the same-day bare control, never a historical constant (#6941's 2026-10-11 drift note) |
| T4 | A declared default reaches the kernel | `ElasticOptions()` default instance vs. the same instance with `max_iter` set to its declared default | identical solves. No Rust literal is consulted (C6 `decoded-param-read` row at 0 for `ElasticOptions`) |
| T5 | An absent declared field is not defaulted | hand-built `ModalOptions` missing `n_modes` | `Failed` with `ComputeInputMalformed` naming `ModalOptions.n_modes`. Today: silently 10 |
| T6 | Unknown enum/string variant | `PressureLoad(direction: "tangential")` from `.ri` | `reify eval` exits 1 with a coded Error naming `PressureLoad.direction` and the supported set. Today: inward normal, exit 0, no diagnostic |
| T7 | Unresolvable pressure face | `PressureLoad(face: "nonexistent")` | a coded Warning in `ComputeOutcome.diagnostics`, not `tracing::warn` alone |
| T8 | Support decoded once | `PinnedSupport` on the modal beam and on an elastic tet body | modal: the existing pinned-face realization (unchanged numbers). Tet: all-DOF clamp (documented). Both routed through one `SupportSpec` decode. Shell-routed body with a user support: a coded Warning naming the dropped support (wiring owner: S16, §13 item 4) |
| T9 | Written equals declared | each builder, production-shaped parts | C4 parity holds. Fixture: injected undeclared field → test names it; stale undeclared-write entry → test names it |
| T10 | Registry reads re-stamp by name | ctor-built instance of a structure declaring trait `T`, overridden into a `T`-typed param while some other structure holds registry id 0 | `validate_param_override` accepts it. Today it is judged against id 0's bounds |
| T11 | Stress symmetry at the Value seam | `.ri`: `von_mises([[1,2,0],[0,0,0],[0,0,0]])` | `reify eval` exits 1 with one coded Error naming the builtin, identically in debug and release. The symmetric control `von_mises([[100,0,0],[0,0,0],[0,0,0]])` is still 100 exactly |
| T12 | Grid layout agreement | `build_slab_sdf(H)` → shell-extract `extract` node | the extracted mid-surface lies in the z = 0 plane within one voxel spacing, with per-vertex thickness ≈ H within one voxel. Today the field is scrambled. The before-image is measured in the leaf's RED test, not assumed |
| T13 | Every shell-extract entry point validates | `SampledField` with `data.len()` one short, and one with NaN spacing, fed to `compute_medial_mask` and `extract_mid_surface` | both return a typed `GridValidationError`-backed error. Today `extract_mid_surface` panics with an index out of bounds |
| T14 | Modal codes, not prefixes | each of the 24 modal diagnostics' producing scenario (existing tests) | each carries its `DiagnosticCode`. Tests assert code identity. `I_MechanismModalNModesTruncated` has `Severity::Info` |

**G6 note.** T3's equality holds by construction, because ctor-built values decode identically. Everything else asserts a rejection, a code or a relation, never a tuned constant. T12's bound is relational: one voxel spacing, the grid's own resolution. Its "today" half is measured, not asserted.

## 6. Resolved design decisions

**D1: defaults have no Rust home; absence is an error (resolves the ruled phrase "defaults taken from the compiled structure_def's default_expr").** The compiler already evaluates every `default_expr` into the instance (§2), so the decoder's correct behaviour on an absent declared field is `Missing`, not a second default. Hand-built test values get their defaults from one test-only helper, `stdlib_default_instance(type_name) -> Value`. It evaluates the zero-arg ctor through a real `Engine`, so the defaults still come from the `.ri` alone. Two alternatives were rejected:

- A runtime defaults table evaluated from `reify_compiler::load_stdlib()` templates (`ValueCellDecl::default_expr`). It would add a production `EvalContext` construction site under S4's INV-EVAL-2 ratchet, and nothing in production needs it.
- Keeping Rust fallbacks with a parity test. That leaves two homes and is the finding itself.

The trampolines' existing graceful-degradation claim for malformed `options` and `supports` fields becomes a coded `Failed`. That claim is the comment above `elastic_static`'s `gate_or_fail!`, and it is pinned by `validate_all_inputs_gate_does_not_panic_on_malformed_options_field`. The change is reachable only from hand-built values, and the pinned test is rewritten to assert the coded `Failed`.

**D2: S15's decoders land first; PDROP builds on them (program-lead ruling 2026-10-12, OQ2 (b)).** S15 does not mint a honored-set mechanism, and β does not wait on PDROP tasks that have not started. β delivers the typed `ElasticOptions` / `ModalOptions` / `BucklingOptions` decoders and deletes the restated fallbacks (`extract_eigen_knobs`, `extract_buckling_options`, the `shell_threshold` local). β is behaviour-preserving for ctor-built values and honors nothing new. Three PDROP tasks then depend on β:

- **#7079** attaches its C1 honored / ignored / not_applicable declaration to β's decoders.
- **#7080** honors `max_iter` / `cg_tolerance` through the decoded `ElasticOptions`, deleting `SOLVER_MAX_ITER`.
- **#7084** declares mechanism_modal's knobs on the decoded `ModalOptions`.

#7079's C3 rule needs the declared default at runtime to decide "differs from default". It derives that default from the `reify_compiler::load_stdlib()` templates (`ValueCellDecl::default_expr`), never from a Rust literal (OQ3 (a)). D1 stands: S15 adds no production defaults table.

**D3: `warm_started` is declared, not deleted.** `param warm_started : Bool = false` is added to `structure def ElasticResult` and documented as solve telemetry beside `iterations` / `converged`. The cache bridge keeps rehydrating `false` (`persistent_cache::ElasticResult` does not store it, by design). Rejected: moving it to `structured_detail`, which would leave the only reader with no `.ri`-visible channel and rewrite a passing e2e for no user gain. The alternative is recorded in §12 OQ1.

**D4: `ModalResult.topology` stays an undeclared write owned by #7097.** S15 neither declares nor deletes it. #7097 owns the `part`/`topology` convergence. The C3 undeclared-write list carries `(topology, #7097)`, and C4 reds if #7097's resolution declares it and the list entry is not removed.

**D5: the registry-free fix lands at the readers as well as the producers (C5).** This is the §2 G6 premise correction. Migrating the 18 Rust `StructureTypeId(0)` producer sites alone would leave every compiler-built instance exposed.

**D6: the asymmetric stress tensor is a coded Error returning `Undef`, not a symmetrization.** Silently symmetrizing would be a plausible wrong answer for a genuinely asymmetric input. The builtin returns `Value::Undef` with a specific `UndefCause`. A `diagnose` classifier arm for `analysis.rs` emits one `Severity::Error` with a new `DiagnosticCode::StressTensorAsymmetric`, following the existing `flexure_diagnose` / `fea_diagnose` registration pattern in `reify-stdlib/src/lib.rs`. S8 later migrates that registration into its typed diagnose-hook column. The f64 kernels keep the documented precondition and one shared `is_symmetric_3x3` for their `debug_assert!`.

**D7: modal codes keep their message prefixes for one release.** Each of the 24 mnemonics becomes a `DiagnosticCode` variant, and the message keeps its leading mnemonic so the 9 out-of-module substring assertions can be migrated to code identity in the same leaf without a flag day. The leaf migrates every substring assertion. The `I_` severity mismatch is fixed by emitting via `Diagnostic::info`.

**D8: partitions follow the measured groups, cut where the seat said.** `fea.rs` becomes `fea/` as follows:

- `mod.rs`: the `eval_fea` dispatcher.
- `grid.rs`: `as_sampled_field`, the `same_grid` adapters, and `TensorShape`, so that `linear_combine` and `to_global` stop importing sideways from the envelope file.
- `cases.rs`, `combine.rs`, `envelope.rs`, `reduce.rs`, `buckling_cases.rs`.

`dynamics/eval.rs` becomes:

- a reader leaf;
- mass properties, folded into the existing `dynamics/mass_props.rs`, which η makes the `MassProperties` owner;
- `ramp.rs`;
- the RNEA groups;
- the sampling driver;
- the dispatcher.

Each test module moves with its production group, and no `pub(super)` widening is introduced to keep a test reaching a sibling's private items (tests stance).

## 7. Disposition table

| Finding | Disposition | Leaf |
|---|---|---|
| fk-34754bed8353 ElasticMaterial readers, `read_scalar_si`, silent zeros | decoder + one error + one `Failed` point | α |
| fk-cee09de8be7a restated option defaults, unread params | options decoders, no default literals; #7079 attaches its declaration to them | β |
| fk-f33ff551c66d support kind decided in many places (weakened, narrowed per skeptic) | one `SupportSpec` decode; shell-route drop made loud | γ |
| fk-2ac688334abf `PressureSpec.direction` dropped (**#8524**) | `PressureDirection` enum + coded unresolvable-face diagnostic; subsumes #8524 | γ |
| fk-a28b1b57ed64 registry-free identity | `registry_free` + `REGISTRY_FREE` + name re-stamp at registry reads | δ |
| fk-b5dc0cd8ec92 ElasticResult writers, `warm_started` | one builder + parity test; `warm_started` declared | ε |
| fk-340a2cbd265b ModalResult / Mode builders, `topology` | one builder each + typed decode + parity; `topology` → #7097 list entry | ζ |
| fk-dc44c395ee01 MassProperties minters/parsers | `MassProperties` owner in reify-stdlib with `to_value`/`from_value` | η |
| fk-d30cf96fc557 `as_f64` copies (**#8559**) | residual copies deleted; drifted variants given explicit policy; subsumes #8559 | θ |
| fk-9082d3765668 modal mnemonics | `DiagnosticCode` per mnemonic; Info severity fix | ι |
| fk-da538013331f stress symmetry | one Value-boundary check, coded Error | κ |
| fk-c02d5d2d8515 `shell_extract_compute_fn` | per-phase error→Diagnostic fns, `?` pipeline, one `Failed` point | λ |
| fk-4ac205f76343 grid axis order, transposed producer, cycle | `SampledField` 3D accessors; `build_slab_sdf` fixed; shell-extract `grid` module | μ |
| fk-90433ba69658 partial grid validation | `validate_regular3d` covers data length + axis geometry for every entry point | ν |
| fk-9bc22225093b `grids_equal`, metadata-copy literals | `same_grid` / `with_data`; `fea.rs` h14 partition | ξ |
| h14 `dynamics/eval.rs` (stays_whole:false, 7) | partition along measured groups | ο |

## 8. Pre-conditions (G3, verified on main 2026-10-12)

- `reify_ir::arg_acceptance` exposes `accept_arg`, `accept_field` (with Option unwrapping and an `Absent` arm), `ArgSpec`, `ArgRejection::message` and the `pressure_spec` / `length_spec` / `time_spec` / `frequency_spec` / `dimensionless_spec` / `force_spec` / … constructors. **Present.**
- `StructureMeta.field_layout` is populated from stdlib templates by `populate_structure_registry` at `Engine::new`, and exercised by `engine_new_populates_structure_registry_from_prelude`. **Present.** C4 needs nothing new.
- `CompiledExprKind::StructureInstanceCtor.defaults` fills every non-supplied param. **Present** (D1's premise).
- `Value::as_f64`. **Present.**
- `DiagnosticCode` is `#[non_exhaustive]`, so new variants are additive. The flexure / fea `diagnose` classifier registration pattern exists. **Present.**
- `shell_boundary::SupportKind { Fixed, Pinned }` and `build_support_bcs`. **Present.**
- **In flight, hard upstream:** **#6922** (dimension-checked-readers γ1/ε/η, pending, under review on `task/6922`). It creates `fea_inputs.rs` / `fea_loads.rs` / `fea_value_shape.rs`, consolidates the three `cell_f64` copies into a `pub` one in stdlib `dynamics::eval`, and introduces `MassPropertiesSi` / `read_mass_properties`. α, γ, δ, η and ο build on its landed shape and re-measure after it lands.
- **Pending, hard upstream:**
  - **#6941** (β/γ2/δ/ζ). It consolidates the trajectory `read_scalar_si` / `field_f64` and touches `modal_ops.rs`'s `read_scalar_si`. α and θ follow it.
- **Downstream, not upstream:** **#7079**, **#7080** and **#7084** (PDROP α/β/ζ) depend on β (D2).
- No grammar change. The one `.ri` edit is D3's `param warm_started : Bool = false`, which reuses existing `param` grammar and is checked with `tree-sitter parse --quiet` at decompose. `grammar_confirmed: true` on every leaf.

## 9. Cross-PRD relationship (G4)

| Artefact / PRD | Direction | Mechanism | Owner |
|---|---|---|---|
| `crates/reify-compiler/stdlib/*.ri` structure defs as the SPOT for defaults | this PRD **owns** (program G4 table) | D1: no Rust default literal; C6 `decoded-param-read` row | **S15**. PDROP (#7079 C3) and PVAC (#7099–#7102) consume |
| `reify_ir::arg_acceptance` | this PRD **owns** (program G4 table) | decoders consume it; any spec constructor a decoder needs is added additively here; `accept_arg` stays frozen | **S15**. S10 (`kernel_validation`, typed SI newtypes) consumes |
| `trampoline-param-drop-closure.md` (#7079–#7085) | **produces** the decoder sites; #7079, #7080, #7084 depend on β | #7079's C1 declaration attaches to β's decoders; #7080/#7084 honor through them | PDROP owns the mechanism, the honors and the `PDROP` gate |
| `result-field-vacuity-closure.md` (#7099–#7102) | **produces** the single builder sites PVAC's C1′ declarations attach to | C3 builders; C4 runtime parity is a complement, not the PVAC gate | PVAC owns `reify-audit --pattern PVAC` (#7102) and the dispositions |
| `dimension-checked-readers.md` (#6922, #6941, #5802) | **builds on** #6922/#6941; **hands** `DecodeFailure::UnsupportedKind` to #5802 | `fea_inputs` → `stdlib_inputs`; `FeaValueShapeError` → `InputDecodeError` | that PRD owns dimension gating and `E_FeaLoadKindUnsupported` |
| #7097 (`ModalResult.part` / `.topology`) | **defers to** | C3 undeclared-write entry `(topology, #7097)` | #7097 |
| **S5** analysis crate | S5 **consumes** | decoders, builders, `into_failed` move with the targets; S5's `.ri @optimized` parity is INV candidate 4's other half | S5 |
| **S16** trampoline factoring | S16 **consumes** | typed request built from C1 decoders; "one result builder" = C3 `build_elastic_result`; shell-route support wiring (§13 item 4) | S16 |
| **S10** `StructureInstanceData` overlays (fk-7b093573458b) | S10 **consumes** C5 | `registry_free` is the one constructor its new typed overlay fields extend | S10 |
| **S6** dynamics (fk-38811e6f7300 MechanismTree, fk-a9b0785fd293 JointKind) | **ordered after** S6's `dynamics/eval.rs` leaves | ο partitions after the open/closed-chain marshalling is one module (the seat's named precondition) | S6 |
| **S8** builtin registry | S8 **consumes** | κ's `analysis` diagnose registration and ξ's moved `fea::diagnose` are rows its typed diagnose-hook column migrates | S8 |
| **S25** gui `engine.rs` | file contention only | θ deletes `scalar_to_f64` before S25's split | S25 |
| **S14** solver crates | no seam | `BucklingKernelOptions::default` / `EigenSolverOptions::default` stay solver-library defaults; β only guarantees no `.ri`-declared knob reaches a kernel through them | S14 |

No new contested-ownership pair is introduced (`phase-3-breadcrumb-map.md` §3).

## 10. Decomposition plan

Phase 1 is the foundation (α, δ, μ). Phase 2 is the vertical slice on one structure family (α ElasticMaterial through all four trampolines, then ε for one builder). Phase 3 covers the remaining decoders and builders. Phase 4 covers primitives and partitions. Phase 5 covers invariants, docs and the PRD close. Every leaf adds tests to **existing** test binaries. The one new gate-resident member is C6's infra test, whose `.sh` wrapper and manifest row land in α's own diff (esc-4914-162). Leaves that move files (μ, ξ, ο) run `reify-audit --pattern PDCHECK` after the move and repoint any live task's `delivered_checks` path they broke. That repoint is three writes per the overlay.

| Label | Task | Modules | Observable signal | Prereqs | Keys · INV |
|---|---|---|---|---|---|
| α #8674 | `stdlib_inputs` foundation: `InputDecodeError` + `into_failed` + `ComputeInputMalformed`; `ElasticMaterial` decoder adopted by elastic_static, buckling, modal_ops, as_printed_material; `modal_ops::read_scalar_si` and input-position silent zeros deleted (geometry, time grid, forcing magnitudes, loss factor, vec3 reads via named specs); C6 member + wrapper + manifest row (`decoded-param-read` for ElasticMaterial, `read-scalar-si` = 0, `modal-silent-zero`, `inline-failed`) | reify-eval, reify-core, tests/infra | T1 + T2 on all four trampolines (Rust, below the compiler); T3 regression floor; ratchet member green on the real tree and red on its seeded fixture | **#6922**, **#6941** | fk-34754bed8353 · INV-SF-6 |
| β #8679 | Options decoders (`ElasticOptions`, `ModalOptions` incl. Rayleigh α/β and `reference_direction`, `BucklingOptions`); no default literal; decodes exactly today's read set (declared-unread params keep their PDROP owners; #7079 attaches its declaration here afterwards); `extract_eigen_knobs` / `extract_buckling_options` fallbacks and the `shell_threshold` local deleted; `stdlib_default_instance` test helper; `decoded-param-read` rows for the three | reify-eval | T4 + T5; `decoded-param-read` rows at 0 for the three options structures; `extract_eigen_knobs` and `extract_buckling_options` absent; `extract_eigen_knobs_reads_fields_and_falls_back` rewritten to assert `Missing` | α (#7079, #7080, #7084 depend on β) | fk-cee09de8be7a · INV-PD-1 (decoder sites for #7079) |
| γ #8683 | Loads + supports decoders: `LoadSpec` (Point / Pressure / Gravity; Traction / BodyForce → `UnsupportedKind`), `PressureDirection` enum, coded unresolvable-face Warning, `SupportSpec { kind: SupportKind, target }` consumed by modal_ops (`DeclaredSupport` string match deleted) and the elastic BC build; shell route emits a coded Warning when user supports are dropped; `decoded-param-read` rows for the load and support structs | reify-eval | T6 + T7 + T8 | α, **#6922** | fk-f33ff551c66d, fk-2ac688334abf · subsumes **#8524** |
| δ #8686 | `StructureInstanceData::registry_free` + `StructureTypeId::REGISTRY_FREE`; 5 consts and the 18 Rust `StructureTypeId(0)` producer sites migrated; `value_type_kind_matches` TraitObject arm and the `validate_param_override` debug check re-stamp by name; `registry-free-literal` row = 0 | reify-ir, reify-eval, reify-stdlib | T10 (red today); `registry-free-literal` at 0 | **#6922** (file overlap) | fk-a28b1b57ed64 |
| ε #8690 | `build_elastic_result` / `elastic_result_from_value` pair used by shell arm, tet arm and the cache bridge; `param warm_started : Bool = false` declared (D3); C4 parity test + fixture self-test; `result-type-literal` row for ElasticResult | reify-eval, reify-compiler stdlib | T9 for ElasticResult; `fea_cold_start_heuristic_e2e` reads `warm_started` from a declared field; `.ri` `result.warm_started` type-checks | δ | fk-b5dc0cd8ec92 · INV-PD-2 |
| ζ #8693 | `build_modal_result` / `build_mode` + typed decode used by transient and displacement_at; the three ModalResult and two Mode builders collapse; `(topology, #7097)` undeclared-write entry; stale "6 fields" docstrings corrected; parity tests; `result-type-literal` rows | reify-eval | T9 for ModalResult + Mode | δ, α | fk-340a2cbd265b · INV-PD-2 |
| η #8697 | `MassProperties` owner in reify-stdlib `dynamics::mass_props` (`to_value` / `from_value`, built on #6922's `read_mass_properties`); `dynamics_ops::assemble_mass_properties` / `inertia_value` and `dynamics_psd::inertia_3x3_from_value` deleted; parity test; `result-type-literal` row | reify-stdlib, reify-eval | T9 for MassProperties; the PSD hook and RNEA read one shape (an existing dynamics e2e unchanged) | δ, **#6922** | fk-dc44c395ee01 · INV-PD-2 |
| θ #8698 | Residual `Value::as_f64` copies deleted (`color_cell_f64`, `bom_report::value_as_f64`, gui `scalar_to_f64`); `tensegrity_crack::scalar_f64` given an explicit policy (route through `accept_arg` with its declared spec, or `as_f64` with the Int arm restored — chosen by its callers' declared types); `value-f64-copy` row | reify-eval, gui/src-tauri | `value-f64-copy` row at its residual baseline; tensegrity crack on an `Int`-valued input no longer reads `None` | α, η, **#6941** | fk-d30cf96fc557 · subsumes **#8559** |
| ι #8702 | Modal `DiagnosticCode` per mnemonic (24); `I_MechanismModalNModesTruncated` via `Diagnostic::info`; all 26 in-module + 9 out-of-module substring assertions migrated to code identity | reify-core, reify-eval, reify-eval tests | T14 | ζ | fk-9082d3765668 · INV-SF-6 |
| κ #8705 | `is_symmetric_3x3` shared by the three kernels; one Value-boundary check in the stress builtins → `Undef` + `StressTensorAsymmetric` via an `analysis` diagnose arm; `symmetry-closure` row = 0 | reify-stdlib, reify-core, reify-expr (classifier dispatch) | T11 (`reify eval` on a committed `tests/prd-gate/fixtures/` asymmetric fixture exits 1 in debug and release) | — | fk-da538013331f · INV-SF-2, INV-SF-6 |
| λ #8706 | One mapping fn per shell-extract phase error type (medial, mid-surface, prune, mesher, segmentation) → `Diagnostic`; `shell_extract_compute_fn` runs on `?` and converts once; code-less `Err(e)` arms get codes; `inline-failed` row for the file | reify-eval | every phase error a test triggers carries a `DiagnosticCode`; `inline-failed` count for the file falls from 13 to 1 | α | fk-c02d5d2d8515 |
| μ #8711 | `SampledField::{flat_index_3d, at_3d, from_fn_3d}` stating axis-0-outermost; `build_slab_sdf` rebuilt on `from_fn_3d`; shell-extract `grid` module holding `sample_at_index` / `normalize3` (breaks the medial↔walk_direction cycle); `interp::index_3d` and `sampled_fd` route through `flat_index_3d`; the six slab fixtures on `from_fn_3d`; `grid-index` row | reify-ir, reify-eval, reify-shell-extract, reify-expr | T12 | — | fk-4ac205f76343 |
| ν #8714 | `validate_regular3d` gains data-length and axis-geometry checks (`GridValidationError` variants absorbing `DataLengthMismatch` / `AxisExtentsOverflow` / `InvalidAxisGeometry`), called by every pub entry point; `MidSurfaceError` wraps `GridValidationError`; the "infallible by construction" comment corrected | reify-shell-extract | T13 | μ | fk-90433ba69658 |
| ξ #8717 | `SampledField::{same_grid, with_data}` (exhaustive destructure); `grids_equal` / `metadata_matches` and the metadata-copy literals routed through them; `fea.rs` → `fea/` along D8; `grid-metadata-copy` row; PDCHECK repoint | reify-ir, reify-stdlib | adding a field to `SampledField` fails to compile `same_grid` (one exhaustive predicate); envelope/linear_combine e2e unchanged; `crates/reify-stdlib/src/fea.rs` absent, `fea/mod.rs` present | μ | fk-9bc22225093b · h14 |
| ο #8718 | `dynamics/eval.rs` partitioned along D8; PDCHECK repoint | reify-stdlib | dynamics e2e unchanged; no new `pub(super)` widening for tests; each moved file makes sense in isolation (h13) | η, **#6922**, **S6** `dynamics/eval.rs` leaves (HELD: `x_seam_wait` S6) | h14 (no finding key) |
| π #8720 | INV candidate 4 (S15's half): `docs/legibility/design-invariants.md` INV-PD-2 enforcement text names the C4 builder parity tests beside PVAC; census table row for "Rust-written result key sets" added as detector-gated; `docs/invariants.md` unchanged (INV-PD-* live in the legibility doc) | docs | committed rows naming the test symbols; no second list (INV-9) | ε, ζ, η | INV-PD-2 (promoted INV cand 4) |
| ρ #8722 | Docs-truth: `crates/reify-mcp/src/tools/chunks/functions.md` `von_mises` family documents the symmetric precondition and the error; `ElasticResult.warm_started` documented where ElasticResult fields are (the `.ri` doc block; chunk if one lists them); discoverability: an author who knows the goal ("why did my stress builtin error", "did my solve warm-start") reaches it from the chunk | reify-mcp chunks, stdlib `.ri` docs | every documented signature compiles in a smoke `.ri`; intent-level findability | ε, κ | — |
| σ #8724 | PRD-close: terminal stamp | this PRD + manifest | committed `SHIPPED` header with landed leaf ids + AS-AUTHORED freeze + LIVE/AS-AUTHORED map | all above | — |

**Docs-truth gate.** Item 1 is ρ and item 4 is folded into ρ. Item 2 (exemplar corpus) and item 3 (cheatsheet index line) are **not applicable**: no leaf introduces or alters an authoring idiom (diagnostics and one telemetry field only), so there is no corpus file for an index line to point at.

**G7 walk (advisory at author time).**

- INV-PD-1: β provides the decoder sites, #7079 provides the one declaration mechanism, and there is no second mechanism.
- INV-PD-2 is strengthened (C3/C4).
- INV-SF-6 holds: every new diagnostic is coded (α, γ, ι, κ, λ).
- INV-SF-2 holds: κ's Error exits nonzero through `reify eval`.
- `nothing-vacuous-and-unowned`: `warm_started` is declared (D3); `topology` is owned (D4); the shell-route support drop is made loud, with its wiring owner per §13 item 4.

No waiver is needed.

## 11. Out of scope

- **Honoring any currently-ignored param.** PDROP owns it (#7080, #7084, #7074, #7075, #7457, #4746, #7263, #7179). β only re-homes reads; PDROP builds its declarations and honors on β's decoders.
- **Dimension gating policy itself.** dimension-checked-readers owns it (#6922, #6941, #5802). This PRD adopts its specs and does not change them.
- **The `reify-audit --pattern PVAC` and `--pattern PDROP` gates.** Those are #7102 and #7085.
- **Wiring user supports into the shell route's synthesized mesh**, which is solver-route work. γ only makes the drop loud (§13 item 4).
- **The trampoline split into typed request + route enum.** That is S16. The `modal_ops.rs` / `elastic_static.rs` h14 partitions are also S16's.
- **Moving compute targets to the analysis crate.** That is S5.
- **The `StructureInstanceData` overlay-field redesign.** That is S10. C5 only centralizes construction.

## 12. Open questions (tactical)

1. **D3 alternative.** If the lead prefers `warm_started` not to be `.ri`-visible, ε moves it to a typed `ComputeOutcome::Completed` diagnostic or `structured_detail` and rewrites `fea_cold_start_heuristic_e2e`. *Suggested:* keep D3. Decide before ε.
2. **Ratchet member granularity.** C6 uses one member with per-primitive rows, where INV candidate 5's checkable form reads "a tests/infra member per primitive". *Suggested:* one member, with each row an independently-failing `unittest` case, so a red names its primitive. Decide in α.
3. **`stdlib_default_instance` location.** It could live in reify-eval behind `test-instrumentation`, or in `reify-test-support`'s `eval-helpers`. *Suggested:* reify-test-support, so other crates' tests can use it. Decide in β.
4. **`tensegrity_crack::scalar_f64` policy.** Restoring the Int arm or dimension-checking depends on whether its callers' declared types are dimensioned. Measure the callers in θ.
5. **`ComputeInputMalformed` vs. an existing code.** No existing `DiagnosticCode` covers a trampoline input-shape failure (checked: `BuiltinArgShapeUnrecognized` is a compile-time typing code with different semantics). *Suggested:* mint it in α.

## 13. Program-lead rulings (2026-10-12; `plans/review-all-program-reify-20261012.md` §Resolved decisions — pass 1)

1. **PVAC.** #7102 stays the PVAC gate, and the program's "the chartered PVAC detector" refers to #7102. S15's C4 is INV-PD-2's runtime half.
2. **PDROP sequencing.** S15's decoders land first. #7079, #7080 and #7084 depend on β (D2).
3. **#7079's C3 default source.** #7079 derives declared defaults from the `load_stdlib()` templates. A note was appended to #7079, and D1 stands.
4. **Shell-route user supports.** S16 owns the wiring (per-route solvers). γ only makes the drop loud.
5. **fk-a28b1b57ed64.** δ stays in S15. It fixes the two registry reads and leaves the compiler placeholder unchanged.
