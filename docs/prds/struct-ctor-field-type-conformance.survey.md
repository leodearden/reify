# Struct-ctor field-type conformance — corpus survey

**Base commit:** `700e931514450e42101722ff0e0f9fc851b28302`
**Tool:** `crates/reify-compiler/tests/harness_compilation_surface/ctor_conformance_corpus_survey.rs`
**Design:** `docs/prds/struct-ctor-field-type-conformance.md` (task β, §8)
**Sites:** 18
**Corpus:** 676 tracked `.ri`; 670 surveyed, 6 not surveyed, 72 partial

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

The **`hint` column is ADVISORY**, derived purely from the (expected, found)
type pair. It is **not** a D9 ruling. PRD §4 D9 defines the split between class
(1) *call-site bug* and class (2) *wrong declared field type* as "per-case
judgment … whichever is the actual bug" and assigns it to **γ**; β does not
pre-empt it. What β does decide mechanically is the `owner` grouping below.

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

| site | def | def source | field | expected | found | code | severity | hint (advisory) | message |
|---|---|---|---|---|---|---|---|---|---|
| `tests/prd-gate/fixtures/dcr_load_ctor_dimension_silent.ri:28` | PointLoad | ctor call-site anchor | force | Real | Scalar[m·kg·s^-2] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | argument 'force' has type 'Scalar[m·kg·s^-2]' but param 'force' requires type 'Real' |
| `tests/prd-gate/fixtures/dcr_load_ctor_dimension_silent.ri:31` | TractionLoad | ctor call-site anchor | traction | Real | Scalar[kg·m^-1·s^-2] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | argument 'traction' has type 'Scalar[kg·m^-1·s^-2]' but param 'traction' requires type 'Real' |
| `tests/prd-gate/fixtures/dcr_solver_load_dropped_dimensioned.ri:34` | PointLoad | ctor call-site anchor | force | Real | Scalar[m·kg·s^-2] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | argument 'force' has type 'Scalar[m·kg·s^-2]' but param 'force' requires type 'Real' |
| `tests/prd-gate/fixtures/dcr_yield_stress_dimension_silent.ri:31` | Steel_AISI_1045 | ctor call-site anchor | yield_stress | Scalar[kg·m^-1·s^-2] | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | argument 'yield_stress' has type 'Scalar[m]' but param 'yield_stress' requires type 'Scalar[kg·m^-1·s^-2]'; pass a dimensioned Pressure literal such as `1kg/m/s^2` |

### non-FEA structure def — γ per-case judgment — 9 site(s)

The recovered name IS a `structure def` declared in the corpus or the stdlib,
and it is not FEA-owned. D9's per-case judgment applies: fix the call site or
the declared field type, whichever is the actual bug — γ's ruling, recorded in
γ's diff. **This is the group to size γ against.**

That check is against ONE GLOBAL namespace — *some* corpus or stdlib file
declares the name, not necessarily one this row's file can see. See named
limitation 3 below before treating a row here as actionable.

| site | def | def source | field | expected | found | code | severity | hint (advisory) | message |
|---|---|---|---|---|---|---|---|---|---|
| `examples/trajectory/printer_print_envelope.ri:172` | TOTSShaper | ctor call-site anchor | acceleration_limit | Scalar[m·s^-2] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | argument 'acceleration_limit' has type 'Real' but param 'acceleration_limit' requires type 'Scalar[m·s^-2]'; pass a dimensioned Acceleration literal such as `1m/s^2` |
| `examples/trajectory/printer_print_envelope.ri:172` | TOTSShaper | ctor call-site anchor | velocity_limit | Scalar[m·s^-1] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | argument 'velocity_limit' has type 'Real' but param 'velocity_limit' requires type 'Scalar[m·s^-1]'; pass a dimensioned Velocity literal such as `1m/s` |
| `tests/prd-gate/fixtures/dcr_material_dimension_silent.ri:24` | Material | ctor call-site anchor | youngs_modulus | Scalar[kg·m^-1·s^-2] | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | argument 'youngs_modulus' has type 'Scalar[m]' but param 'youngs_modulus' requires type 'Scalar[kg·m^-1·s^-2]'; pass a dimensioned Pressure literal such as `1kg/m/s^2` |
| `tests/prd-gate/fixtures/dcr_reader_ctor_dimension_silent.ri:28` | MassProperties | ctor call-site anchor | mass | Scalar[kg] | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | argument 'mass' has type 'Scalar[m]' but param 'mass' requires type 'Scalar[kg]'; pass a dimensioned Mass literal such as `1kg` |
| `tests/prd-gate/fixtures/dcr_reader_ctor_dimension_silent.ri:29` | ZVShaper | ctor call-site anchor | target_frequency | Scalar[s^-1] | Scalar[rad·s^-1] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | argument 'target_frequency' has type 'Scalar[rad·s^-1]' but param 'target_frequency' requires type 'Scalar[s^-1]'; pass a dimensioned Frequency literal |
| `tests/prd-gate/fixtures/dcr_reader_ctor_dimension_silent.ri:31` | AsPrintedOptions | ctor call-site anchor | line_width | Scalar[m] | Real | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | argument 'line_width' has type 'Real' but param 'line_width' requires type 'Scalar[m]'; pass a dimensioned Length literal such as `1m` |
| `tests/prd-gate/fixtures/dcr_reader_ctor_dimension_silent.ri:32` | FDMCouponOverride | ctor call-site anchor | ex | Scalar[kg·m^-1·s^-2] | Scalar[m] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | argument 'ex' has type 'Scalar[m]' but param 'ex' requires type 'Scalar[kg·m^-1·s^-2]'; pass a dimensioned Pressure literal such as `1kg/m/s^2` |
| `tests/prd-gate/fixtures/dcr_shaper_frequency_dimension_silent.ri:37` | ZVShaper | ctor call-site anchor | target_frequency | Scalar[s^-1] | Scalar[rad·s^-1] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | argument 'target_frequency' has type 'Scalar[rad·s^-1]' but param 'target_frequency' requires type 'Scalar[s^-1]'; pass a dimensioned Frequency literal |
| `tests/prd-gate/fixtures/r3b_displacement_at_selector_grammar.ri:59` | StepForce | ctor call-site anchor | at | Selector | String | `ArgTypeMismatch` | Warning | selector field given a string — typed ctor such as face(b, "x_max") or vertex(b, "tip") is the usual replacement | argument 'at' has type 'String' but param 'at' requires selector type 'Selector' |

### name recovered, but it is not a known structure def — needs manual triage — 2 site(s)

An identifier was recovered at the diagnostic's anchor, but it is not a
`structure def` declared anywhere in the corpus or the stdlib. Recovery reads
an identifier followed by `(`, which cannot distinguish a ctor from a plain
function call, so these are typically FUNCTION names reaching the survey from
a non-ctor path (selector composition, overload resolution) — the `severity`
and `message` columns show which. Held out of the actionable group rather than
sized into it. **Triage manually before touching.**

| site | def | def source | field | expected | found | code | severity | hint (advisory) | message |
|---|---|---|---|---|---|---|---|---|---|
| `crates/reify-eval/tests/fixtures/selectors/bt1_wrong_kind_union.ri:15` | union | ctor call-site anchor | — | — | — | `SelectorKindMismatch` | Error | no mechanical hint — γ per-case judgment | selector composition kind mismatch: cannot compose FaceSelector and EdgeSelector |
| `crates/reify-eval/tests/fixtures/selectors/bt6_kind_typed_param.ri:24` | needs_face | ctor call-site anchor | — | — | — | `SelectorKindMismatch` | Error | no mechanical hint — γ per-case judgment | no matching overload for needs_face(EdgeSelector), candidates: needs_face(FaceSelector) -> Int |

### unattributed def — needs manual triage — 3 site(s)

No def name could be attributed. The `def source` column gives the
machine-derived reason PER ROW rather than asserting one cause for the group:
known shapes that land here include the sub `=` per-arg anchor and param
default-initializer checks, both of which anchor the label somewhere other
than a `Def(` call site. Deliberately its own group: folding an
unattributable site into the touchable pile is the one classification error
with a real cost. **Triage manually before touching.**

| site | def | def source | field | expected | found | code | severity | hint (advisory) | message |
|---|---|---|---|---|---|---|---|---|---|
| `tests/prd-gate/fixtures/curvature_rad_literal.ri:12` | — | unrecovered: identifier not followed by `(` | kc | Scalar[m^-1] | Scalar[rad·m^-1] | `ArgTypeMismatch` | Warning | no mechanical hint — γ per-case judgment | argument 'kc' has type 'Scalar[rad·m^-1]' but param 'kc' requires type 'Scalar[m^-1]'; pass a dimensioned AbsorptionCoeff literal |
| `tests/prd-gate/fixtures/raw_lambda_material_field_rejected.ri:21` | — | unrecovered: identifier not followed by `(` | material | — | — | `TypeNotConformingToTrait` | Error | no mechanical hint — γ per-case judgment | type 'Field<<error>, AnisotropicMaterial>' does not conform to trait 'ConstitutiveLaw' required by param 'material' |
| `tree-sitter-reify/test/fixtures/mv-2-priv-param.ri:4` | — | unrecovered: identifier not followed by `(` | rated_torque | Scalar[m^2·kg·s^-2·rad^-1] | Int | `ArgTypeMismatch` | Warning | dimensioned scalar field given a bare number — a dimensioned literal (e.g. 1m/s) is the usual replacement | argument 'rated_torque' has type 'Int' but param 'rated_torque' requires type 'Scalar[m^2·kg·s^-2·rad^-1]'; pass a dimensioned Torque literal such as `1m^2*kg/s^2/rad` |

## Coverage and limitations

Of 676 tracked `.ri` members, **670 were surveyed** and **6 were not**. A further **72** were surveyed only PARTIALLY. Both are listed below rather than dropped: a bounded sweep that does not state what it skipped reads as full coverage and would under-size γ.

### Not surveyed (contributed no sites)

| file | reason |
|---|---|
| `crates/reify-cli/tests/fixtures/bracket_parse_error.ri` | `parse-error` |
| `docs/prds/v0_6/fixtures/indexed_sub_instantiation_surface.ri` | `parse-error` |
| `gui/test/fixtures/broken_syntax.ri` | `parse-error` |
| `tests/prd-gate/fixtures/adt_mirror_of_arm.ri` | `parse-error` |
| `tests/prd-gate/fixtures/arrow_type.ri` | `parse-error` |
| `tests/prd-gate/fixtures/unit_middot_mul.ri` | `parse-error` |

### Partially surveyed (sites collected, but the file also failed to compile)

| file | reason |
|---|---|
| `crates/reify-cli/tests/fixtures/bracket_compile_error.ri` | `compile-error` |
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
| `tests/prd-gate/fixtures/collection_sub_at_placement_rejected.ri` | `compile-error` |
| `tests/prd-gate/fixtures/compiler_type_hygiene_integration_gate.ri` | `compile-error` |
| `tests/prd-gate/fixtures/compiler_type_hygiene_mul_scale_guard_defeat.ri` | `compile-error` |
| `tests/prd-gate/fixtures/compiler_type_hygiene_mul_vec_silent_int.ri` | `compile-error` |
| `tests/prd-gate/fixtures/compiler_type_hygiene_trait_args_silent_accept.ri` | `compile-error` |
| `tests/prd-gate/fixtures/compose_middle_type_mismatch_rejected.ri` | `compile-error` |
| `tests/prd-gate/fixtures/compose_one_arg_rejected.ri` | `compile-error` |
| `tests/prd-gate/fixtures/curvature_rad_literal.ri` | `compile-error` |
| `tests/prd-gate/fixtures/dcr_fn_force_param_already_rejects.ri` | `compile-error` |
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

1. **Inline Rust-string `.ri` fixtures are not file-enumerable.** The task's
second half — the Rust test suite's inline fixtures and goldens — lives inside
`const SOURCE: &str = r#"…"#` literals, which `git ls-files` cannot reach and
which could only be swept by changing the compiler (out of scope for this
read-only survey). Their coverage is **transitive, and stated as such rather
than claimed**: the `--scope all --profile both` merge gate is green at the
base commit above, and the landed α/ε gates
(`no_example_emits_ctor_field_conformance_diagnostics`, the
`struct_ctor_field_conformance_tests` suite) already assert on the
ctor-conformance codes.
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
