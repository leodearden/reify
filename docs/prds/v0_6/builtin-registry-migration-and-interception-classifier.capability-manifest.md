# Capability manifest — `builtin-registry-migration-and-interception-classifier.md`

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/builtin-registry-migration-and-interception-classifier.md`
(program `review-all-reify-20261010`, stream S8). Drafted in author mode (pass 1), 2026-10-13.
Machine-readable twin: `docs/prds/v0_6/builtin-registry-migration-and-interception-classifier.capability-manifest.yaml`
(task ids stamped by `commit_planning` on 2026-10-13: S8-α1 #8888, S8-α2 #8889, S8-β1 #8890, S8-β2 #8891, S8-β3 #8892, S8-γ #8893, S8-δ1 #8894, S8-δ2 #8895, S8-ε #8896, S8-ζ #8897, S8-η #8898, S8-μ #8899, S8-ν1 #8900, S8-ν2 #8901, S8-ξ #8902, S8-ο #8903, S8-π #8904, S8-ρ #8905, S8-θ #8906, S8-ι #8907, S8-κ #8908, S8-λ #8909, S8-σ #8910, S8-τ #8911, S8-υ #8912, S8-φ #8913, S8-ω #8914).

> **Code anchors** verified against main `a4a8862bcc` (2026-10-13); every S8 source path is
> byte-identical to the program anchor `a030ee7528`. Cite-by-symbol throughout; every
> `delivered_check` is pattern-anchored on a code construct and was confirmed **red today** (an
> `absent` pattern that matches at main, or a `present` pattern / `path` that does not).
> Fixture outputs below came from `target/debug/reify` built 2026-10-12 15:19; the only source
> commit to the probed crates since then is a reify-eval `cache.rs` prose amend (`0724c2b427`),
> which touches no typing or eval path the fixtures exercise.

## Batch-wide bindings

| Check | Evidence | Verdict |
|---|---|---|
| No novel grammar | `grammar_confirmed=true` batch-wide (S8-π changes the AST type, not the grammar). The eleven new fixtures `tests/prd-gate/fixtures/s8_*.ri` parse with `tree-sitter parse --quiet` from `tree-sitter-reify/`, exit 0, 0 ERROR nodes (2026-10-13) | **PASS** |
| New fixtures are inert | All eleven are new basenames in `tests/prd-gate/fixtures/`, in neither `_RUST_COUPLED_RI_FIXTURES` nor `_GUI_COUPLED_RI_FIXTURES`; adding a fixture does not escalate pre-commit | **PASS** |
| Behaviour floor exists | reify-compiler/reify-expr/reify-stdlib/reify-eval integration harnesses, `examples_smoke`, the prd-gate corpus and the workspace build are gate members; every leaf carries PRD §5 row 17 | **PASS** |
| Language surface changes carry docs-truth | S8-υ covers the chunks, exemplar, cheatsheet index and discoverability for every user-visible change (geometry typing and `E_GeometryCallInValuePosition`, `W_UserFnShadowsBuiltin`, the new geometry and sweep rejection codes, `safety_factor`'s yield rule, Frame plane projections); wired as real prereqs | **PASS** |
| Gate-test drift-guard | New infra rows go into the existing `tests/infra/adoption-ratchet.manifest` (member `test_infra_adoption_ratchet.sh` already registered); any new gate-resident Rust test carries its nextest / wallclock registrations in the same diff (overlay rule, stated in PRD §8) | **PASS** |
| Adoption-ratchet grammar | Rows are `pattern-id \| ERE \| path \| ceiling \| owner leaf`; Rust `//` lines count, so each ERE is construct-anchored (PRD C9) | **PASS** |
| G7 (reify INV-SF family) | Advisory: S8-α1 records specific `UndefCause` codes (INV-SF-1 strengthened); S8-η retires a misstated placeholder type (INV-SF-5); every `Rejection` carries a `DiagnosticCode` (INV-SF-6); no waiver | **PASS** |

## Recorded pre-state (fixtures and pins; commands from the repo root)

| Fixture / pin | Command | rc | Output today |
|---|---|---|---|
| `tests/prd-gate/fixtures/s8_geometry_value_position.ri` | `reify eval` | 0 | `via_let = 2`; `via_inline = undef` + `note: … via_inline is undef (because: op contract failed (OpContractViolation))`; `check` rc 0 "All constraints satisfied." |
| `tests/prd-gate/fixtures/s8_geometry_inline_overload_error.ri` | `reify check` | 1 | `error: parameter 'pl' declared `Scalar[m]` but its initializer evaluates to `Real`…`; `error: no matching overload for takes_geo(Real), candidates: takes_geo(Geometry) -> Int`; `error: argument 'pl' has type 'Real' but param 'pl' requires type 'Scalar[m]'…` |
| `tests/prd-gate/fixtures/s8_geometry_annotations.ri` | `reify check` / `reify eval` | 0 / 0 | check silent; eval `pr = undef`, `a = undef` (OpContractViolation notes), `ll`/`li` = `<Geometry: …>` |
| `tests/prd-gate/fixtures/s8_geometry_ctor_conformance.ri` | `reify check` / `reify eval` | 0 / 0 | silent; `vialet.name`/`len`/`origin` all hold `<Geometry: …>` |
| `tests/prd-gate/fixtures/s8_geometry_ctor_conformance_inline.ri` | `reify check` | 1 | `error: argument 'name' has type 'Real' but param 'name' requires type 'String'`; `error: argument 'len' has type 'Real' …`; `origin` not rejected |
| `tests/prd-gate/fixtures/s8_geometry_arith_cmp.ri` | `reify check` / `reify eval` | 0 / 0 | check: `INDETERMINATE …#constraint[0]: operator undefined for these operand kinds` + warning, "No constraints violated (1 indeterminate)."; eval: `bad1`, `bad2`, `cmp` = `undef`, `vol = 0.000000001 m^3` |
| `tests/prd-gate/fixtures/s8_frame_datum_planes.ri` | `reify eval` | 0 | `i_xy = plane(…)`, `i_yz = undef`, `i_zx = undef`; every `f_*` (frame from `self.x`/`self.z`) = `undef`; `g_o`/`g_x`/`g_z`/`g_xy` evaluate, `g_yz`/`g_zx` = `undef` |
| `tests/prd-gate/fixtures/s8_user_fn_shadows_builtin.ri` | `reify check` / `reify eval` | 0 / 0 | check silent; eval `c = 111`, `fl = 333`, `m = undef` (user `map_or` swallowed by the intercept), `m_option = 11` |
| `tests/prd-gate/fixtures/s8_sweep_malformed.ri` | `reify eval` | 0 | eight malformed calls `= undef`, each with only the generic OpContractViolation note; `ok_sweep` a 3-snapshot list, `ok_dim` a sweep_dim map |
| `tests/prd-gate/fixtures/s8_geometry_builtin_silent_undef.ri` | `reify eval` | 0 | `f`, `f_arity`, `p`, `t`, `v` all `undef` with only the generic note; check silent |
| `tests/prd-gate/fixtures/s8_safety_factor_nonpositive_yield.ri` | `reify eval` | 0 | `sf_pos = 2.5`, `sf_neg = -2.5`, `sf_zero = 0` |
| `examples/flexures/yield_warning.ri` (S8-α2 floor) | `reify check` | 0 | `warning: W_FLEXURE_YIELDING: …`, `warning: W_FLEXURE_PRB_OUT_OF_RANGE: …`, one `info: W_FLEXURE_FATIGUE_CHECK_MISSING: …` |
| `examples/process/std_process_dfm.ri` (S8-α2 floor) | `reify check` | 1 | four `constraint …#1[0] violated` errors; `W_DFM_DRAFT` / `E_DFM_DRAFT`; `E_DFM_BUILD_VOLUME` / `I_DFM_BUILD_VOLUME` / `W_DFM_BUILD_VOLUME`; "Some constraints violated." |
| CoM density fallback (S8-α2 floor) | — | — | no committed example or fixture triggers it today; S8-α2 authors one |
| `examples/fea_multi_case_bracket.ri` (S8-λ floor) | `reify eval` | 0 | `result = {"cases": {"operating": ElasticResult {…}, "overload": …, "transport": …}}`, each converged; three shell-threshold fallback warnings |

## Per-leaf bindings

### S8-α1 — Typed builtin outcome; one settle point; every caller
| Capability | Evidence | Verdict |
|---|---|---|
| one entry point to retype | `reify_stdlib::eval_builtin(name, args) -> Value` chains the registry hoist + 23 `eval_*` families | PASS |
| caller set is closed | production callers: reify-expr `eval_expr` catch-all, `analysis.rs` ×2, `dual_eval.rs::eval_dual_builtin`; reify-eval `engine_constraints::measure_gdt_conformance`, `geometry_ops::fold_pose_chain`; plus `registry_parity_tests.rs` and test modules (`git grep eval_builtin`) | PASS |
| a typed sink exists for the coded cause | `UndefCause::OpContractFailed { code, span }` and `EvalContext::undef_causes`; `push_op_contract_failure` pushes the generic code today | PASS |
| `ArgRejection` exists to convert | `reify_ir::arg_acceptance::ArgRejection` | PASS |
| delivered | `pub struct Rejection` present in `crates/reify-core/src/diagnostics.rs`; `fn eval_builtin(…) -> BuiltinOutcome` present in `crates/reify-stdlib/src/lib.rs` | — |

### S8-α2 — Success-path advisories (dfm, flexure, snapshot)
| Capability | Evidence | Verdict |
|---|---|---|
| the three hooks emit on success | dfm verdicts, `W_FlexureYielding` / `W_FlexurePrbOutOfRange` / fatigue Info, `SnapshotCenterOfMassDensityFallback` (seat rt-intercept §D) | PASS |
| constraint-path DFM consumer exists | `engine_constraints::measure_dfm_rules` calls `dfm_diagnose` at 4 sites | PASS |
| S6 σ₀ #8846 moves the CoM group first | dependency edge (PRD §6) | PASS |
| delivered | `fn emit_(dfm\|flexure\|snapshot)_diagnostics` absent in `crates/reify-expr/src` | — |
| flexure re-classifier deleted (gap-fill 2026-10-16, K6-D5) | `flexures/common.rs::classify_geometry_invalid` + its 13-name positional layout table re-derive `E_FlexureGeometryInvalid` from a rejected ctor's raw args at main `c7bbfe2519`; the `parse_*_inputs` return `Option` | PASS |
| delivered (gap-fill 2026-10-16) | `fn classify_geometry_invalid` absent in `crates/reify-stdlib/src/flexures` | — |
| enumerated corrections (gap-fill 2026-10-16, K6-D5) | `examples/flexures/yield_warning.ri` and `examples/process/std_process_dfm.ri` stay byte-identical. The flexure input classes that are silent Undef today gain coded rejections from `FlexureInputError` (naming field and value): missing E, non-positive width or E, a non-unit axis, wrong arity. These are the only flexure diagnostic changes S8-α2 may make | — |

### S8-β1 — Geometry: the 19 diagnosed names become rejections
| Capability | Evidence | Verdict |
|---|---|---|
| one producer exists for value + reason | `DatumFault` / `AffineMapFault` classifiers shared by eval and diagnose arms | PASS |
| delivered | `fn (datum_fault_diagnostic\|diagnose_bbox_corners)` absent in `crates/reify-stdlib/src` | — |

### S8-β2 — Geometry: the 28 undiagnosed names gain coded rejections
| Capability | Evidence | Verdict |
|---|---|---|
| the silent set is real | `s8_geometry_builtin_silent_undef.ri` eval: all `undef`, generic note only | PASS |
| delivered | manual — behaviour on the fixture (one coded Error per call); no stable grep expresses "every rejecting arm is coded" | — |

### S8-β3 — Stdlib `geometry.rs` partition
| Capability | Evidence | Verdict |
|---|---|---|
| tests reach public seams | record: 373 `eval_builtin` + 87 diagnose calls through public seams; a few reach `super::` helpers | PASS |
| delivered | `^mod tests \{` absent in `crates/reify-stdlib/src/geometry.rs` | — |

### S8-γ — FEA, dynamics, analysis rejections
| Capability | Evidence | Verdict |
|---|---|---|
| hooks exist and are re-derivations | `fea::diagnose` mirrors `linear_combine`'s ten checks; `dynamics::eval::diagnose` "best-effort attribution"; #8705 adds `analysis` | PASS |
| upstream moves | S15 ξ #8717 (`fea/combine.rs`), κ #8705, S6 ρ #8845 are prerequisites | PASS |
| delivered | `as (fea\|dynamics)_diagnose` absent in `crates/reify-stdlib/src/lib.rs` | — |

### S8-δ1 — Stackup, tolerancing, orientation; post-hoc axis deleted
| delivered | `fn emit_undef_builtin_diagnostics` absent in `crates/reify-expr/src`; `as (stackup\|tolerancing\|orientation\|geometry)_diagnose` absent in `crates/reify-stdlib/src/lib.rs` | — |
|---|---|---|

### S8-δ2 — Sweep rejections
| Capability | Evidence | Verdict |
|---|---|---|
| the silent exits are real | `s8_sweep_malformed.ri`: eight `undef`, generic note only | PASS |
| S6 ο #8843 lands first | dependency edge | PASS |
| delivered | `Rejection` present in `crates/reify-stdlib/src/sweep.rs`. Accepted gap: proves the type is used, not that every exit is coded; behavioural cover is the fixture | — |

### S8-ε — Intercept column and `classify_call`
| Capability | Evidence | Verdict |
|---|---|---|
| `BuiltinId` in `CompiledExpr` | producer: registry β #6002 (upstream edge) | PASS |
| the intercepted set and guards | 20 names; guards `Always`, `Arg0Lambda` (`fn_field`), `Arg0Field`, `Arg0FieldArg1BoundingBox` cover every measured arm (seat rt-intercept §B; critic) | PASS |
| no intercepted name is `.ri`-declared | critic: no `fn sample/restrict/flat_map/generate/…` in `crates/reify-compiler/stdlib` | PASS |
| delivered | `fn (reduction_kind_of\|field_reduction_kind)\(` absent in `crates/reify-expr/src/dual_eval.rs`; `pub fn classify_call\(` present in `crates/reify-expr/src` | — |

### S8-χ — Smoothness column; dual_eval name tables deleted (gap-fill 2026-10-16, K9-D4)
| Capability | Evidence | Verdict |
|---|---|---|
| the second home is real | `dual_eval.rs` `is_kink_builtin`, `is_differentiable_builtin` (13 smooth builtins), `builtin_partials` are `(name, arity)` matches; `crates/reify-builtins/src` has no `differentiab`/`kink` hit (review R96, fk-819257c68b82) | PASS |
| S8-ε and #8480 do not cover it | #8896's checks name only `reduction_kind_of`/`field_reduction_kind`; #8480 moves the three tables within reify-expr | PASS |
| delivered | `fn (is_kink_builtin\|is_differentiable_builtin\|builtin_partials)\(` absent in `crates/reify-expr/src`; `enum Smoothness` present in `crates/reify-builtins/src` | — |

### S8-ζ — Stdlib intrinsics; one shadowing rule
| Capability | Evidence | Verdict |
|---|---|---|
| provenance at resolution | `merge_prelude_functions` builds `resolution_functions` (compiler `expr.rs`, `functions_phase.rs`) | PASS |
| the shadowing defect is real | `s8_user_fn_shadows_builtin.ri`: `m = undef` (user `map_or` swallowed), `clamp`/`floor` replaced silently | PASS |
| S8-λ removes `solve_load_cases` from the intercept first | dependency edge | PASS |
| delivered | `fn is_intercepted_user_fn\(` absent in `crates/reify-expr/src/dual_eval.rs`; `\.any\(.f. f\.name ==` absent in `crates/reify-compiler/src/expr.rs`; `UserFnShadowsBuiltin` present in `crates/reify-core/src/diagnostics.rs` | — |

### S8-η — Geometry calls typed by kind; lowering-unreachable geometry calls are an error
| Capability | Evidence | Verdict |
|---|---|---|
| one placeholder site | ladder arm 9 only (seat geo-type Q1) | PASS |
| no consumer depends on the placeholder | `result_type` not hashed; consumers enumerated (seat geo-type Q2) | PASS |
| `sweep`/4 needs its own type | `List<StructureRef("Snapshot")>` per `joint_signatures`; live in `examples/kinematic/` | PASS |
| corpus impact | 0 of 265 examples change `reify check`; the only inline constructor geometry is `std_tolerancing_surface.ri` (enumerated correction) | PASS |
| rejection fires (G6 branch 4) | post-state asserted by the leaf on the four committed fixtures; pre-state recorded above | PASS (pre-state) |
| delivered | `fn geometry_fn_result_type\(` present in `crates/reify-compiler/src`; `GeometryCallInValuePosition` present in `crates/reify-core/src/diagnostics.rs` | — |

### S8-μ — Discriminant dispatch
| Capability | Evidence | Verdict |
|---|---|---|
| descriptor table exists with discriminants | `GEOMETRY_OP_DESCRIPTORS` (55 rows, 60 names), `GeometryOpDiscriminants` | PASS |
| delivered | `=> unreachable!\(` absent in `geometry_{boolean,curve,transform,modify}.rs` | — |

### S8-ν1 — Arg indices, preconditions, trait-inference arms from the descriptor
| delivered | `GeometryBuiltin` present in `crates/reify-compiler/src/geometry_traits_inference.rs` | — |
|---|---|---|

### S8-ν2 — Compiler `geometry.rs` partition
| delivered | manual — structural (one exhaustive `match GeometryBuiltin` calling per-family lowering functions); no single-line grep expresses it | — |
|---|---|---|

### S8-ξ — Typed Modify arguments
| Capability | Evidence | Verdict |
|---|---|---|
| key vocabulary measured | 24 mint sites in `geometry_modify.rs`; per-kind keys enumerated (seat cmp-map §E) | PASS |
| S13 δ #8750 creates `geometry_ops/compile/` first | dependency edge | PASS |
| delivered | `pub enum ModifyArgs` present in `crates/reify-compiler/src/types.rs` | — |

### S8-ο — Parameterized-builtin-type table
| delivered | `fn (resolve_parameterized_builtin_type_with_subst\|resolve_type_alias_expr_to_dim_with_subst)\(` absent in `crates/reify-compiler/src/type_resolution.rs` | — |
|---|---|---|

### S8-π — Operators at the syntax boundary
| Capability | Evidence | Verdict |
|---|---|---|
| enum exists to move | `reify_ir::{BinOp, UnOp}` (Debug, Clone, Copy, PartialEq, Eq, Hash); reify-ast depends only on reify-core | PASS |
| delivered | `op: String` absent in `crates/reify-ast/src/ast.rs` | — |

### S8-ρ — Curvature face overload by type
| delivered | manual — the leaf's own fixture (`curvature(extremal_by_bbox(b)[0], p)` types the matrix form) | — |
|---|---|---|

### S8-θ — Datum-projection table in reify-ir
| Capability | Evidence | Verdict |
|---|---|---|
| reify-ir reachable from all three consumers | Cargo: reify-compiler, reify-expr, reify-eval all depend on reify-ir | PASS |
| drift is real | `s8_frame_datum_planes.ri` (above) | PASS |
| SCC constraint | reify-ir row {boundary_attachment, expr, geometry, value}; the new module imports `value`, is not imported by it | PASS |
| delivered | `path` `crates/reify-ir/src/datum_projection.rs` present; `const SELF_DATUM_PROJECTION_MEMBERS` absent in `crates/reify-eval/src` | — |

### S8-ι — `FieldPayload`
| Capability | Evidence | Verdict |
|---|---|---|
| no serialisation or hash format | `Value` derives only `Debug, Clone` | PASS |
| consumer set enumerated | seat rt-values §A + critic (gui `engine.rs`, reify-test-support builders, test literals) | PASS |
| delivered | `pub enum FieldPayload` present in `crates/reify-ir/src/value.rs`; `items\.len\(\) == 7` absent in `crates/reify-expr/src/lib.rs` | — |

### S8-κ — Tensor-kernel table; non-positive yield rejected
| Capability | Evidence | Verdict |
|---|---|---|
| divergence real | `s8_safety_factor_nonpositive_yield.ri`: `-2.5`, `0` | PASS |
| delivered | `fn project_safety_factor_sampled\(` absent in `crates/reify-expr/src/field_reductions.rs`; `SafetyFactorNonPositiveYield` present in `crates/reify-core/src/diagnostics.rs` | — |

### S8-λ — One `solve_load_cases` home
| Capability | Evidence | Verdict |
|---|---|---|
| the dispatch path exists | `try_compute_dispatch` called from `eval_user_function_call` and `dual_eval`; `ComputeDispatch` trait in reify-ir | PASS |
| byte-identical floor | `examples/fea_multi_case_bracket.ri` output above | PASS |
| delivered | `fn (eval_solve_load_cases\|invoke_solve_elastic_static)\(` absent in `crates/reify-expr/src` | — |

### S8-σ — One keyword table in reify-syntax
| Capability | Evidence | Verdict |
|---|---|---|
| grammar has no reserved-word API | `tree-sitter-reify/grammar.js` declares no `word:` rule | PASS (the table carries `Reserved`/`Contextual`, pinned by a parse test) |
| delivered | `path` `crates/reify-syntax/src/keywords.rs` present; `crate::completion::\{BODY_KEYWORDS` absent in `crates/reify-lsp/src/references/` | — |

### S8-τ — reify-expr `lib.rs` partition
| delivered | `path` `crates/reify-expr/src/context.rs` present | — |
|---|---|---|

### S8-υ — Docs-truth
| delivered | manual — chunk text and the exemplar's compile are asserted by the leaf and `examples_smoke` | — |
|---|---|---|

### S8-φ — [MILESTONE] hoisting PRD
| delivered | manual — a human `/prd` session; no code deliverable | — |
|---|---|---|

### S8-ω — PRD close
| delivered | `^Status: \*\*SHIPPED` present in the PRD | — |
|---|---|---|

## Gap-fill amendments (2026-10-16)

Rulings from `plans/gap-fill-deliberation-reify-20261016.md`. Every new or changed check below was confirmed red at main `c7bbfe2519` with `git grep -E`.

- §K6-D5: S8-α2 #8889 gains `flexure-geometry-reclassifier-deleted` (`fn classify_geometry_invalid` absent in `crates/reify-stdlib/src/flexures`) and the enumerated flexure corrections above (three writes: this row, the `.yaml`, #8889's `metadata.delivered_checks`).
- §K9-D4: new leaf S8-χ #9289 (smoothness column), two checks above.
