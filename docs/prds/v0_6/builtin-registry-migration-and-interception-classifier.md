# PRD: Builtin registry migration and one interception classifier — typed builtin outcomes, one call classifier, geometry calls typed Geometry

Status: **active (pass 1)**. Authored 2026-10-13 by the S8 `/prd` session of program
`review-all-reify-20261010` (program doc `plans/review-all-program-reify-20261012.md`, stream
**S8 Builtin registry migration and one interception classifier**, wave 2, mode spawn, size L,
risk medium). Shape: **B + H** (contracts §4, boundary-test sketch §5). Milestone: v0_6.

> **Code anchors** verified against main `a4a8862bcc` (2026-10-13). The program's freshness
> anchor is `a030ee7528` (2026-10-10). `git diff a030ee7528 main` over `crates/reify-compiler/src`,
> `crates/reify-expr/src`, `crates/reify-stdlib/src`, `crates/reify-builtins`, `crates/reify-lsp/src`,
> `crates/reify-ir/src`, `crates/reify-syntax/src`, `crates/reify-ast`,
> `crates/reify-eval/src/geometry_ops.rs` and `crates/reify-eval/src/registry_drift_tests.rs` is
> **empty**, so every finding measurement holds at both commits. Cite-by-symbol throughout; the
> few figures quoted with a line range are dated by this header. Re-locate at implementation time.

This PRD **finishes** `docs/prds/v0_6/builtin-signature-registry.md` (the registry PRD). That
document stays the single home of the row-shape contract (§7.1) and the I-REG invariants; this
PRD owns its legs β..ω (program G4 row), amends it where a ruling below changes a leg (§3 D1),
and adds what the registry PRD never covered: the typed builtin outcome, the call classifier,
geometry value typing, and the satellite tables the review found.

**Conventions.** This PRD's own leaves are labelled `S8-α1` … `S8-ω`. A registry leg is always
written with its id (`registry β #6002`, `registry ω #6014`), never by bare letter. **`A ← B`
means A depends on B** (B lands first); it is the only arrow used for dependencies. Landing
order between streams is written as a list ("S7, S9, S8, S12").

---

## §0 — Provenance and rulings

Program ruling *S8 Builtin registry migration and one interception classifier* (human,
2026-10-12; do not relitigate): proceed as a spawn stream finishing the builtin-signature-registry
PRD — one reify-builtins row per builtin carrying static facts (name, arity, arg slots, result
type) **plus** a typed intercept-kind / diagnose-hook column consumed by reify-expr (reify-expr
gains a reify-builtins dependency); the 22-arm ladder, 30 name slices, `geometry_arg_indices`,
`compile_geometry_call_inner`, selector maps, LSP completion/hover signatures and the
reserved-keyword list derive from the rows. The stream row adds: builtins return a typed
rejection so `Undef` and its diagnostic are produced at one point in reify-expr and the post-hoc
`diagnose` classifiers go; a typed `classify_call` replaces `eval_expr`'s string-guarded
intercept arms and `dual_eval`'s hand mirror; the datum-projection table lives once in reify-ir
(*Contradictions* ruling); operators parsed to `BinOp`/`UnOp` at the syntax boundary; one
expression-depth bound; typed `FieldPayload` variants; `KBool` truth tables as the one Kleene
home; and the heuristic-14 partitions of reify-expr `lib.rs` (8), compiler `geometry.rs` (7) and
stdlib `geometry.rs` (3).

Rulings that bind this PRD, by name in the program doc: *S8*, *Contradictions* (datum projection
in reify-ir under S8), *Invariant candidates* (candidate 5 extract-with-ratchet is repo-wide),
*Weakened findings* (fk-a494d2285710 and fk-07c78c4ca710 re-read with their skeptic notes; both
stand at weakened weight), *Filing stance*, and the pass-1 rulings *S9 Q3* (S6 mints
`RelationKind`, S9 λ owns `RelationSig`, S8 reads it) and *S13 Q1* (fk-07c78c4ca710 is S8's;
the selector helper map moves with the registry τ8 leg). Lead amendment: S6 χ #8851, held on
this stream's typed diagnose-hook contract, is released by the lead once S8-α1 is filed.

Session rulings (Leo, 2026-10-13) are recorded in §11 and applied in §3.

Finding keys owned (23 + 2 prior-run keys re-observed by this run):
fk-33801a7c7a36, fk-935b6eca2b50, fk-5a6c2aff4589 (high), fk-ffa4ba4c2d0b, fk-eeebffcc4a83,
fk-ea33d4b5d69d, fk-2e03b9202f6d, fk-a494d2285710 (weakened), fk-d402f7243603,
fk-be87290e126e (high), fk-a72f4510f36b, fk-561a5a490532, fk-7bfe957c7f09 (high),
fk-ecc0b4fb9d6b, fk-76e1dc7c434e, fk-7e3c3d10ae46, fk-eca29487b2c9, fk-419ebb996480,
fk-81f949615552, fk-4bd71b633033 (filed #8558), fk-e187a67f4b32, fk-2845dd9262b4 (filed #8549),
fk-07c78c4ca710 (weakened); prior keys fk-d50200501a51 (`language-pipeline|…/expr.rs|h3`, the
ladder, still present) and fk-c02915c1952e (`constraints-and-geometry|…/geometry_ops.rs|h11`,
the selector maps, still present). Each is carried by exactly one leaf or adopted task in §8.

Heuristics applied (dark-factory `docs/code-quality.md`): **11 SPOT** (name tables, intercept
tables, datum projections, kernels, diagnose twins, parameterized-type tables, keyword lists —
the stream's spine), **3 orthogonal dimensions** (one mechanism per axis: name→result type,
name→intercept, failure→diagnostic), **12 structured data instead of meaningful strings**
(operator strings, string-keyed Modify args, positional field payloads), **10 clear invariants
uniformly enforced** (exhaustive matches replace disjointness-by-test and `unreachable!` on
user input), **1 informative names** and **6 well-defined purpose** (`is_reserved_keyword`, the
`dimensionless_scalar()` placeholder that misstates a value's kind), **4 small function scopes**
and **14 no file too large** (the 1,581-line `compile_geometry_call_inner`; the three
partitions), and the Comments stance (the 70.5%-prose `builtin_arg_slots`, the 49%-prose
`geometry::diagnose`, 21 stale `lib.rs:NNN` pointers in `dual_eval.rs`).

## §1 — Goal and consumers (G1)

After this PRD lands:

- A builtin's failure is one typed value. `eval_builtin` returns
  `Result<Evaluated, Rejection>`; reify-expr turns a `Rejection` into `Undef` + one coded
  diagnostic + a coded `UndefCause` at one point, and success-path advisories ride on
  `Evaluated`. No builtin's reason for failing is re-derived by a second function.
- Which calls `eval_expr` intercepts is one typed column on each reify-builtins row, read by
  `eval_expr`, `dual_eval` and the compiler through one `classify_call`; a missed mirror is a
  compile error, not a silent wrong primal.
- A geometry-constructor call is typed `Geometry` (`List<Snapshot>` for the kinematic `sweep`),
  so overload resolution, conformance, operators and annotations see the truth. A geometry call
  passed as an argument to a user function or a structure constructor — where realization
  lowering does not reach today and the value is a silent `undef` — is a coded error with a
  "bind it with `let`" hint (D6 lists every position and its disposition).
- The datum-projection table, the per-kind tensor kernels, the parameterized builtin types, the
  operator vocabulary, the reserved keywords, the Modify argument vocabulary and the geometry
  dispatch each have one home that its consumers read.

| Mechanism | Consumer (G1) |
|---|---|
| `Rejection` / `Evaluated` / `BuiltinOutcome` + reify-expr settle point (S8-α1) | every family-conversion leaf (S8-α2, β1, β2, γ, δ1, δ2, κ); every `eval_builtin` caller (reify-expr `lib.rs`, `analysis.rs`, `dual_eval.rs`; reify-eval `engine_constraints::measure_gdt_conformance`, `geometry_ops::fold_pose_chain`, `registry_parity_tests`); **S6 χ #8851** (TOTS diagnostics on the value-eval path); **S15 κ #8705 / ξ #8717** (their diagnose registrations land here); **S12** (coded builtin diagnostics); user-visible: `reify check`/`reify eval` diagnostics (§5 rows 1–5) |
| Intercept column + `reify_expr::classify_call` (S8-ε) | `eval_expr`, `dual_eval` (both call it), the compiler's intercept handling (S8-ζ); user-visible: §5 row 6 |
| Stdlib-intrinsic stamp + one shadowing rule (S8-ζ) | `eval_expr`'s `UserFunctionCall` arm, `dual_eval`, the compiler's `filter`/`cost`/`generate` handling, reify-eval `structural_query`; user-visible: `W_UserFnShadowsBuiltin` (§5 row 7) |
| `geometry_fn_result_type` + `E_GeometryCallInValuePosition` (S8-η) | overload resolution, conformance, operator typing, annotation checks, trait inference, LSP hover; registry τ9 rows (#6944) and τ5 (#6007) carry the same result types; user-visible: §5 rows 8–11 |
| `reify_ir::datum_projection` table (S8-θ) | compiler MemberAccess typing, `eval_datum_projection`, reify-eval's self-datum path, S3's `expr_classify` feature-member set; user-visible: §5 row 12 |
| `FieldPayload` (S8-ι) | every `Value::Field` producer and decoder (reify-expr, reify-eval, reify-stdlib `fea/`, `gui/src-tauri/src/engine.rs`, reify-test-support builders) |
| tensor-kernel table (S8-κ) | `sample_tensor_grid_at_point`, `project_sampled_tensor_windows`, the stdlib `safety_factor`, the field-form `compute_safety_factor`; user-visible: §5 row 13 |
| one `solve_load_cases` home (S8-λ) | nested calls through the existing `try_compute_dispatch`; user-visible: §5 row 14 |
| `GeometryBuiltin` / discriminant dispatch (S8-μ, ν1, ν2) | `compile_geometry_call_inner`'s satellites, `geometry_arg_indices`, `check_profile_preconditions`, `geometry_traits_inference`'s per-op arms |
| `ModifyArgs` (S8-ξ) | reify-eval's modify compilers, `deps.rs`, `engine_build.rs` arg walkers |
| `PARAMETERIZED_BUILTIN_TYPES` (S8-ο) | the one parameterized resolver and the one alias-to-dimension resolver; #7736 (LSP type-position completion) may read it |
| operator enums at the syntax boundary (S8-π) | `operator_typing`, `type_resolution` const folding, `geometry::extract_count_constraint`, the `expr.rs` value-pow check |
| `reify_syntax::KEYWORDS` (S8-σ) | LSP rename gate (`is_valid_rename_identifier`), reify-audit `RI_KEYWORDS` consumers (pdoccover) |
| curvature face classification by type (S8-ρ) | `geometry_query_arg_aware_result_type` for `curvature` |

Out-of-batch consumers named by other streams: **S12** (same wave; compiler landing order S7,
S9, S8, S12) consumes S8-α1's `Rejection`→`Diagnostic` codes and registry β #6002's `BuiltinId`
in `CompiledExpr`; **S23** (wave 4) consumes the row `basis` column as the home of the
per-builtin rationale prose that `builtin_arg_slots` and `geometry::diagnose` carry today.

## §2 — Current state at main (measured 2026-10-13, `a4a8862bcc`)

**Registry.** `reify-builtins` holds 7 rows (parse 2, analysis 5), all `BindingKind::EvalBuiltin`;
`BindingKind` declares `ExprIntercept`, `EnginePostProcess`, `GeometryOp`, `CompileOnly` with no
rows; `ArgSlot` has only `Any`. reify-compiler and reify-stdlib depend on it; reify-eval only as a
dev-dependency (`registry_parity_tests.rs`, registry ψ #6013, landed); **reify-ir and
reify-expr do not**. `CompiledExprKind::FunctionCall` carries `ResolvedFunction { name,
qualified_name }` and no `BuiltinId`; eval re-resolves by string. Registry α #6001 and ψ #6013
have landed; the other legs are filed: β #6002, combiners #6943 (τ1 numeric + τ6 complex),
#6944 (τ7 datums/affine/list/field + τ8 queries/selectors + τ9 relations/geometry-ops), #6945
(τ3 joints), τ2 #6004, τ4 #6006, τ5 #6007, π #6012, ω #6014, λ #6015, ρ #6016.
`scripts/assert-crate-dag.sh` rule B3 (owned by `docs/prds/core-ast-ir-layering.md` §3) allows
reify-ir workspace dependencies ⊆ {reify-core, reify-ast}.

**The result-type ladder** (`expr.rs::compile_expr_guarded_with_expected_inner`, the
`NoUserFunctions` arm): 22 arms plus the terminal first-arg fallback; arm 18 is the registry;
arm 9 is `is_geometry_function → Type::dimensionless_scalar()`. Its precedence comment omits 5
arms and cites the disjointness test in `units.rs`; the test is
`unresolved_function.rs::builtin_name_families_are_pairwise_disjoint` over the 25-row
`BUILTIN_NAME_FAMILIES` with 5 `INTENTIONAL_OVERLAPS`. 30 `*_NAMES` slices live in
reify-compiler (16 in `units.rs`, which S7 ε #8622 moves to `builtin_families.rs`). The
`FunctionCall` arm intercepts `some`, `filter`, `cost`, `generate` by bare name before overload
resolution; three carry an ad-hoc `!functions.iter().any(|f| f.name == …)` user-shadow check.
A user `fn` named like a builtin replaces the builtin wholesale (its other arities disappear):
`tests/prd-gate/fixtures/s8_user_fn_shadows_builtin.ri`.

**Geometry typing.** The placeholder is assigned at exactly one site (ladder arm 9). A list
literal of geometry calls inherits `List<Real>`; the arity-4 kinematic `sweep` is typed `Real`
although it returns a list of snapshots. Consumers that undo it: three
`is_geometry_function` name sniffs in `conformance/mod.rs`, the
`geometry_traits_inference.rs::is_geometry_operand` disjunct, and two `entity.rs` pass-2
`cell_type` overrides. `geometry.rs::is_geometry_let` / `merge_branches` are AST-level routing
to realization lowering and are **not** placeholder compensation. `result_type` is not hashed
(`FunctionCall` content hash = tag + qualified name + argument hashes), so nothing depends on
the placeholder. Recorded at main (fixtures in `tests/prd-gate/fixtures/`, outputs in the
manifest): `takes_geo(box(..))` and `param pl : Length = box(..)` are rejected only because the
call is typed `Real`; `pick(box(..))` silently picks `pick(x: Real)` and evaluates `undef`;
`param pr : Real = box(..)`, `let ll : Length = cylinder(..)`, `let li : Int = sphere(..)` and an
unannotated `fn mk(w) { box(w,w,w) }` are silent; a let-bound geometry passes `String`,
`Length` and `Point3` constructor params silently; `constraint box(..) > 0` is an exit-0
INDETERMINATE warning. The only corpus sites with an inline geometry call as a
constructor/function argument are in `examples/tolerancing/std_tolerancing_surface.ri`
(`feature:`/`datum_refs:` arguments, which evaluate `undef`).

**Geometry dispatch.** `compile_geometry_call_inner` spans 1,581 lines and matches 65 names in
about 60 arms; boolean/curve/transform/modify satellites re-match the names and end in
`_ => unreachable!(…)` (4 sites); `geometry_arg_indices` lists 35 names with a silent
`_ => &[]` default. reify-ir `GEOMETRY_OP_DESCRIPTORS` has 55 rows / 60 names with `parent_role`
and `GeometryOpDiscriminants`; reify-compiler production code never reads it.
`GEOMETRY_FUNCTION_NAMES` (65) = descriptor names (60) ∪ {`rounded_box`, `rounded_rect`,
`zone_cylinder`, `zone_annulus`, `zone_profile`} (builder-routed composites).
`CompiledGeometryOp` (reify-compiler `types.rs`) carries `args: Vec<(String, CompiledExpr)>` on
every variant but `Boolean`; `Modify` mints 24 string keys in `geometry_modify.rs`, found again by
string compare in reify-eval's modify compilers.

**Interception and diagnostics.** `reify_stdlib::eval_builtin(name, args) -> Value` chains the
registry hoist then 23 `eval_*` families; a name miss and a rejection both surface as `Undef`.
`eval_builtin`'s production callers: reify-expr `eval_expr` (the `FunctionCall` catch-all),
`analysis.rs` (two re-entries), `dual_eval.rs::eval_dual_builtin`; reify-eval
`engine_constraints::measure_gdt_conformance` and `geometry_ops::fold_pose_chain`; plus the
registry parity harness and test modules. `eval_expr`'s `FunctionCall` arm intercepts 20 names
(23 named arms + `std::__interp_render`) before `eval_builtin`; `fn_field` fires only on a
`Lambda` operand, `restrict`, the four analysis names and the eight field reductions only on a
`Field` operand. The `UserFunctionCall` arm intercepts four families by bare name and arity
(`solve_load_cases/5|6`, the ten `option_recovery` combinators, `map_or/3`, `map_err/2`); a
user `fn map_or(a, b, c)` evaluates `undef` because the intercept swallows it.
`dual_eval.rs` restates the user-call gates (`is_intercepted_user_fn`) and the reduction table
(`reduction_kind_of`, `field_reduction_kind`); 21 `lib.rs:NNN` tokens there are stale. Nine
diagnose hooks (47 name slots): six run only after `Undef` (stackup 3, fea 1, geometry 19,
dynamics 2, tolerancing 1, orientation 1) via `emit_undef_builtin_diagnostics`; three emit on
**success** too (dfm 5 — the verdict *is* the diagnostic; flexure 14 — `W_FlexureYielding`,
`W_FlexurePrbOutOfRange`, the once-per-sink fatigue Info; snapshot 1 — the CoM density fallback)
via `emit_{flexure,dfm,snapshot}_diagnostics`. reify-eval's
`engine_constraints::measure_dfm_rules` calls `dfm_diagnose` at four sites to re-derive DFM
verdicts from evaluated cell values on the constraint path. Every non-intercepted builtin
`Undef` records the generic `UndefCause::OpContractFailed { code: OpContractViolation }`.
reify-stdlib production code holds 708 `Value::Undef` sites (geometry 109, joints 103,
snapshot/orientation/fea 67 each, sweep 31); `eval_geometry` dispatches 47 names,
`geometry::diagnose` 19 of them; `eval_sweep` has 29 `Undef` exits and no diagnose path (each
prints only the generic note). S15 κ #8705 adds a tenth hook (`analysis`,
`StressTensorAsymmetric`) before this stream runs; S15 ξ #8717 moves `fea::diagnose` into
`fea/combine.rs`; S6 σ₀ #8846 moves snapshot's centre-of-mass group into `dynamics/com.rs`;
S6 ο #8843 makes some `sweep` failures coded diagnostics. The precedent for one producer
serving both value and diagnostic exists only in `geometry.rs` (`AffineMapFault`,
`DatumFault`); reify-ir's `arg_acceptance::ArgRejection` is the closest typed rejection.

**Satellite tables.** Datum projection has three homes: compiler
`datum_projection::datum_projection_result_type` + `DATUM_PROJECTION_MEMBERS` (13),
`reify_expr::eval_datum_projection`, reify-eval `SELF_DATUM_PROJECTION_MEMBERS` (8) + the
self-datum evaluator (S3 β #8687 moves `FEATURE_DATUM_PROJECTION_MEMBERS` into
`expr_classify`); `Frame.yz_plane`/`.zx_plane` are typed `Plane` but evaluate `undef` on every
frame, and a `frame_at` result built from `self.x`/`self.z` projects `undef` for every member
(#6100); reify-ir is a direct dependency of all three crates. `Value::Field` stores its payload
in `lambda: Arc<Value>` whose shape depends on `source` (14 `FieldSourceKind`s; `SafetyFactor`
List[field, yield], `Restricted` List[inner, region], `AsPrintedZones` a 7-list with a nested
7-number params list); the doc table says `Imported` holds `Undef`, the code holds a
`SampledField`; the `Composed` list form has no production producer; `Value` derives only
`Debug, Clone` (no serde, no hash). The four tensor-analysis kinds' per-window kernels are
written in `analysis.rs` and again in `field_reductions.rs`; `safety_factor` with yield ≤ 0
returns a finite value from the builtin (`-2.5`, `0`) and from `sample()`, and `Undef` from
the reductions. `type_resolution.rs` writes the 12-name parameterized-builtin table four times
(predicate, two matchers, test list) and also twins the alias-to-dimension resolver; the subst
builtin matcher lacks the base matcher's `DimSlotClass` handling for `Scalar`/`Vector3`/`Point3`.
AST operators are `String`s (`reify_ast::ExprKind::BinOp { op: String }`); `resolve_binop`/
`resolve_unop` convert, and five more sites re-test raw strings; typed `BinOp`/`UnOp` exist only
in reify-ir, which depends on reify-ast. `is_reserved_keyword` tests completion's 26 keywords;
reify-audit `RI_KEYWORDS` lists 47 (spec §2.10 + `joint`); `grammar.js` has no `word:` rule and
treats several spec keywords as contextual. The curvature face overload detects faces by
`FACE_PRODUCING_SELECTOR_NAMES` (4) although 11 selectors are typed `Selector(Face)`.
`solve_load_cases` has four homes (`fea_multi_case.ri` stub bodies, the `solver::multi_case`
trampoline, reify-expr `eval_solve_load_cases`/`resolve_load_case_options`/
`invoke_solve_elastic_static`, a permanent `Undef` arm in `fea.rs`); the reify-expr copy
evaluates `solve_elastic_static`'s contract body, bypassing `@optimized` dispatch, and fabricates
a field-less `ElasticResult` on any `Undef`.

**h14 partitions** (record `h14_measurements`, `stays_whole: false` for all three):
reify-expr `lib.rs` (9,970 lines) — context, value ops, method/datum, overload lookup,
intercepts (cycle until a typed classifier exists), sampling (cycle on `apply_lambda`), core
evaluator, tests; compiler `geometry.rs` (6,755) — selector/let predicates, cross-sub AST
helpers, conditional hoisting, profile preconditions, rounded-corner synthesis, dispatch core,
tests; stdlib `geometry.rs` (9,905; 7,290 of it the test module) — datum construction, the
eval/diagnose twin, tests. The sixth `expr.rs` partition (the `FunctionCall` ladder) is S8's by
S7 ι #8631; it collapses with registry ω #6014.

## §3 — Resolved design decisions

**D1 — This PRD owns the registry legs; the registry PRD stays the contract home.** The filed
registry legs are adopted, not refiled (§8 "Adopted"). Finding keys whose remedy *is* a
registry leg are stamped onto that leg. This PRD amends `builtin-signature-registry.md` in the
same commit where a ruling changes a leg: §3 decision 4 gains the intercept column (D3), §4's
geometry value-typing exclusion is struck (D6; its "66" corrected to 65), §7.1 gains the
`intercept` field and points here for the outcome contract, §9's τ7/τ9 bullets record that
field-op intercept rows are S8-ε's and that geometry rows carry D6's result types, and §10 OQ3
is decided (D2). No row-shape fact is restated in this PRD.

**D2 — `BuiltinId` rides inline in `CompiledExpr`; reify-ir and reify-expr depend on
reify-builtins.** Registry OQ3 (inline field vs side table) is decided inline: S8-ε (the
classifier), S8-ζ (intrinsic stamps) and #6944 (descriptor keyed by id) all read the id from
the IR, and a side table would be a second lookup with its own staleness. reify-builtins
depends only on reify-core and strum, so the edge adds no cycle (`dag_invariant.rs` keeps
reify-builtins a leaf). It does breach `assert-crate-dag.sh` B3 as written: registry β #6002
widens B3 to {reify-core, reify-ast, reify-builtins} and amends
`docs/prds/core-ast-ir-layering.md` §3 B3 in the same diff (details note at decomposition).
Whether the id joins the content hash stays registry β #6002's question.

**D3 — The intercept column (ruling: typed intercept kind consumed by reify-expr).** A row
gains `intercept: Option<Intercept>`. `Intercept { kind: InterceptKind, when: OperandGuard,
undef: UndefPolicy }`. `InterceptKind` names what reify-expr does instead of calling
`eval_builtin`, one variant per family: field calculus, field sample, field construction,
field analysis, field reduction (kind × extent), lambda application, interpolation render.
`OperandGuard` (`Always`, `Arg0Lambda`, `Arg0Field`, `Arg0FieldArg1BoundingBox`) states the
operand shape under which the intercept fires — the conditional intercepts the I-REG-1 seed
ledger (`SEED_STRING_DISPATCH_LEDGER`) records as unexpressible today. `UndefPolicy` (`Strict`,
`ObservesUndef`) places `__interp_render` ahead of the strict-`Undef` short-circuit. A name
intercepted unconditionally is an `ExprIntercept` row; a name intercepted only on some operand
shapes is a row of its own binding (`EvalBuiltin` for the analysis names and the reductions)
carrying the column, and falls through to that binding when the guard does not fire. The column
is static data; the guard is evaluated in one place,
`reify_expr::classify_call(id: BuiltinId, args: &[Value]) -> Option<InterceptKind>`, which
`eval_expr` and `dual_eval` both call. The field-cell shadow lookup (a call to a name whose cell
holds a `Field`) stays positional; it is not name-keyed.

**D4 — The diagnose half is a typed outcome, not a row column (session ruling Q1).**
`eval_builtin` returns `BuiltinOutcome = Result<Evaluated, Rejection>` (C1). reify-expr's one
settle point turns `Err(r)` into `Value::Undef`, `r` as a diagnostic, and
`UndefCause::OpContractFailed { code: r.code }` (INV-SF-1: the specific code, never the generic
one), and `Ok(e)` into `e.value` with `e.advisories` pushed. Success-path diagnostics (dfm
verdicts, flexure warnings, the CoM fallback, S6's TOTS outcomes) are advisories; post-Undef
classifiers become rejections at the arm that rejects. reify-expr's own intercept handlers
return `BuiltinOutcome` too and go through the same settle point. This realises the ruling's
"diagnose hook" as the return type: each builtin carries its own diagnostics, so no name-keyed
second axis remains and the rejection work does not wait on any family's row migration. Every
`Rejection` carries a `DiagnosticCode` (INV-SF-6): the code-less tolerancing and `affine_scale`
diagnostics gain codes as their families convert. `reify_ir::arg_acceptance::ArgRejection`
(S15's seam) converts into a `Rejection` through one `From` impl in reify-ir, so the two
rejection types meet at one point instead of competing. A family not yet converted wraps its
`Value` as `Evaluated::legacy`, which keeps today's generic cause; the count of `legacy` wraps
is the migration ledger (C9), and the families outside this PRD's findings (joints, numeric,
complex, matrix, construct, loads, supports, mechanism, tensegrity, trajectory, list, trig,
linalg) stay `legacy` — §9.

**D5 — Stdlib intrinsics are stamped at compile time; one shadowing rule.** The
`UserFunctionCall` intercept families are `.ri`-declared functions, which rows never claim
(I-REG-5). `reify_builtins::STDLIB_INTRINSICS` maps `(name, arity) → StdlibIntrinsic` for the
option-recovery combinators, `map_or/3` and `map_err/2` (`solve_load_cases` leaves the set,
D10); `unwrap_or/2`, `or_else/2` and `fallback/2` are declared in both `option_recovery.ri` and
`result.ri`, so the key is the name and arity and the gate is the resolution's origin. The
compiler stamps `CompiledExprKind::UserFunctionCall { intrinsic: Option<StdlibIntrinsic>, .. }`
only when static resolution chose a stdlib (prelude-merged) declaration; that provenance exists
at resolution time (`merge_prelude_functions`) and is threaded to the IR. The stamp wins over
eval's runtime re-resolution by argument types. reify-expr's `classify_user_call` reads the
stamp, so a user `fn map_or` is never replaced at eval. The compiler's `filter`/`cost`/
`generate` handling moves after user-function resolution and keys on the resolved `BuiltinId`,
deleting the three `!functions.iter().any(…)` checks. A user `fn` whose name is a registry row
or a stdlib intrinsic gets a coded Warning `W_UserFnShadowsBuiltin` naming the shadowed builtin
(#8578's defect); resolution keeps choosing the user function. `some(…)` stays a language
constructor.

**D6 — Geometry calls are typed by kind; a geometry call where lowering does not reach is a
coded error (session ruling Q4).** Ladder arm 9 becomes `geometry_fn_result_type(name, argc)`:
`Geometry` for every `GEOMETRY_FUNCTION_NAMES` call except the arity-4 kinematic `sweep`, which
is `List<StructureRef("Snapshot")>` (matching `joint_signatures`). One predicate
`is_kinematic_sweep(name, argc)` replaces the arity rule duplicated in `geometry.rs` and
`conformance/mod.rs`. `conformance/mod.rs::arg_type_is_unverifiable` treats `Geometry` as
unverifiable only against a `Geometry`, trait or trait-object parameter (otherwise the
let-bound and inline constructor forms stay silent, §5 row 9). The three conformance name sniffs
and the traits-inference disjunct are deleted; the two `entity.rs` overrides become
`debug_assert_eq!` on the compiled type (heuristic 10: the invariant stated twice, never undone);
the `entity.rs` geometry-let branches check the `let` annotation.
Positions and their disposition:

| position of a geometry-constructor call | disposition |
|---|---|
| `let` or `Geometry`-typed `param` initializer; geometry-list `let` (list literal, list helper, its lambda bodies) | lowered today — unchanged |
| geometry argument of another geometry call; argument of a geometry query (hoisted as `__geoq_N`) | lowered today — unchanged |
| argument of a user-function call | **`E_GeometryCallInValuePosition`** (hint: bind it with `let`) |
| argument of a structure constructor (value `let` or `sub`) | **`E_GeometryCallInValuePosition`** — after S8-η's premise probe confirms realization lowering does not reach `sub` geometry arguments; if it does, `sub` arguments are exempt and the task text says so |
| non-geometry-annotated `let`/`param` initializer; operator operand | rejected by the annotation / operand-kind type errors the typing fix makes reachable — no second diagnostic |
| user-function body or return; lambda body outside a geometry-list `let` | unchanged (`undef` stays); owned by φ's hoisting PRD and by #5428 (`.ri` fns returning Geometry in geometry position) |

Leo's ruling: hoisting such calls (as `__geoq_N` hoists query arguments) is the eventual answer;
it does not fit gracefully inside a type-hygiene leaf, so it is a separate PRD milestone
(S8-φ). `Geometry + Int` (`infer_binop_type`'s `Add`/`Sub` has no operand-kind guard) stays
silent: follow-up candidate, §9.

**D7 — One datum-projection table in reify-ir.** `reify_ir::datum_projection` holds rows
`{ receiver: ReceiverKind, member: DatumMember, codomain: Codomain, binding: ProjectionBinding }`
with `ProjectionBinding = Value(fn(&Value) -> Option<Value>) | SelfDatum | FeatureDatum`.
Compiler typing reads `codomain` (and the `Unavailable`/`Ambiguous` dispositions with their
hints); `eval_datum_projection` reads `Value` bindings; reify-eval's self-datum path and S3's
`expr_classify` feature-member set read the `SelfDatum`/`FeatureDatum` rows. A member typed but
not evaluated becomes unrepresentable: every `Resolved` row names its binding. `Frame`'s
`yz_plane`/`zx_plane` therefore evaluate; the normal stays a `Value::Vector` per #6100. The
module imports `value` and must not be imported by it (reify-ir SCC row
{boundary_attachment, expr, geometry, value} does not grow).

**D8 — `FieldPayload` replaces `(source, lambda)`.** `Value::Field { domain_type,
codomain_type, payload: FieldPayload }` with one variant per source kind: `Lambda`,
`Sampled(SampledField)`, `Imported(SampledField)`, `Derived { op: DerivedOp, of: Arc<Value> }`
for gradient/divergence/curl/laplacian/von-Mises/principal/max-shear,
`SafetyFactor { of, yield_strength }`, `Restricted { inner, region }`,
`AsPrintedZones(ZonePayload)` with a named `ZoneParams` struct. `FieldSourceKind` survives only
as `payload.kind()` for consumers that need the tag. The `Composed` list form, which no
production code produces, is deleted. One constructor per variant; decoders destructure.
`Value` carries no serde or hash derive, so no cache or serialisation format changes.

**D9 — One tensor-kernel table; yield ≤ 0 is rejected everywhere (session ruling Q3).**
`analysis` holds one per-kind table (C6) consumed by `sample_tensor_grid_at_point` and
`project_sampled_tensor_windows` (including `safety_factor`, which today hand-rolls its own
unwrap). The validity policy rejects a non-positive yield on every path: the stdlib
`safety_factor` and the field-form `compute_safety_factor` return a coded `Rejection`
(`SafetyFactorNonPositiveYield`) through the settle point, and `sample()` and the reductions
over such a field return `Undef`. This subsumes #7847, whose suggested "keep the divergence as a
reduction-side guard" the ruling overrides.

**D10 — `solve_load_cases` has one home: the `solver::multi_case` trampoline.** reify-expr's
`eval_solve_load_cases`, `resolve_load_case_options`, `invoke_solve_elastic_static`, its
`UserFunctionCall` gate, `dual_eval`'s mirror entry and reify-stdlib's `"solve_load_cases" =>
Value::Undef` arm are deleted. A nested call goes through the existing `try_compute_dispatch`:
with a dispatcher attached (the constraint and dependent-cell paths) it runs the trampoline — a
real multi-case solve where the reify-expr copy produced field-less stubs, a cost change S8-λ's
text enumerates; without one it body-evaluates its `.ri` stub, the rule for every `@optimized`
function, so the no-dispatcher result moves from "map of field-less ElasticResults" to "empty
`MultiCaseResult`". Direct cells are unaffected (cell-level ComputeNode path).

**D11 — Geometry dispatch keys on the descriptor discriminant.** The router parses a name once:
`GeometryBuiltin::from_name(name) -> Option<GeometryBuiltin>` with
`GeometryBuiltin = Op(GeometryOpDiscriminants) | Composite(CompositeKind)` (the five
builder-routed composites). Satellites take the discriminant and match exhaustively; the four
`unreachable!` arms and the inner `name → BooleanOp` match go. `geometry_arg_indices`,
`check_profile_preconditions` and `geometry_traits_inference`'s per-op arms derive from the
descriptor's `parent_role` and the composite table; the silent `&[]` default goes.
`compile_geometry_call_inner` becomes one exhaustive `match GeometryBuiltin` whose arms call
per-family lowering functions, along the h14 groups. `from_name` is a transitional I-REG-1
string site: when #6944 lands the GeometryOp rows, the descriptor keys its `names` by
`BuiltinId` (D2) and both `from_name` and the bridge test go — that step is #6944's, amended at
decomposition.

**D12 — Modify carries typed per-kind arguments.** `CompiledGeometryOp::Modify { kind, target,
args: ModifyArgs }` with one variant per `ModifyKind` and named `CompiledExpr` fields (e.g.
`Shell { thickness, open_faces: Option<CompiledExpr> }`, `Draft { faces: Option<_>, angle,
plane }`); reify-eval's modify compilers destructure; generic walkers (`deps.rs`,
`engine_build.rs`, `prehydrate_inline_curated_selector_args`) read `ModifyArgs::exprs()`. The
other string-keyed variants keep their shape — follow-up candidate, §9.

**D13 — One parameterized-builtin-type table; substitution as a parameter.**
`PARAMETERIZED_BUILTIN_TYPES: &[ParamBuiltin { name, arity, slots: &[SlotKind], build }]` drives
one `resolve_parameterized_builtin_type(name, args, ctx, subst: Option<&Subst>)` and one
alias-to-dimension resolver with the same optional substitution; `is_parameterized_builtin_name`
reads the table; the two `_with_subst` twins and the test list are deleted. The `DimSlotClass`
handling applies on both paths (the alias-DFS path gains it — a behaviour change enumerated in
S8-ο's text). Applies after S7 μ #8643 has threaded `ResolutionCtx` through these functions.

**D14 — Operators are enums from the syntax boundary.** `BinOp`/`UnOp` move from reify-ir to
reify-ast with a `cfg_attr(feature = "serde")` derive (reify-ir re-exports them; downstream
paths unchanged; B5 "ast must not depend on ir" unaffected). `reify_ast::ExprKind::BinOp { op:
BinOp, .. }`; reify-syntax `lower_binary_expr`/`lower_unary_expr` parse the token once
(`BinOp::from_token`); `is_comparison_op` becomes `BinOp::is_comparison`; `resolve_binop`,
`resolve_unop` and the five raw-string sites are deleted. The source spelling (`and` vs `&&`)
is recoverable from the span if a consumer ever needs it.

**D15 — One keyword table in reify-syntax (session ruling Q2).** `reify_syntax::KEYWORDS:
&[Keyword { word, kind: Reserved | Contextual }]`, pinned by a parse-behaviour test (each
`Reserved` word fails to parse as a binding name, each `Contextual` word parses) and by a
parity test against the spec §2.10 table. The LSP rename gate rejects `Reserved` words; the
completion lists stay UX tables over the same words; reify-audit's `RI_KEYWORDS` consumers read
`KEYWORDS` (reify-audit gains a direct reify-syntax dependency; it already has one transitively
through reify-test-support, #6356). The table is grammar vocabulary, not a builtin fact, so it
is not a reify-builtins row.

**D16 — Curvature classifies faces by type.** `is_surface_producing_arg` reads
`Type::Selector(SelectorKind::Face)` on the `ResolveSelector` arm; only `adjacent_faces`
(still `List<Geometry>`) keeps a name check; `FACE_PRODUCING_SELECTOR_NAMES` shrinks to it and
the stale `geometry_query_arg_aware_result_type` doc is corrected.

**D17 — Partitions follow the record's groups and wait for the code that dissolves their
cycles.** reify-expr `lib.rs` splits after the intercept tangle is gone (the record says a clean
intercepts split needs the typed classifier); no partition may add a reify-expr row to
`scripts/module-scc-baseline.json`. Compiler `geometry.rs` splits with S8-ν2. Stdlib
`geometry.rs` splits with S8-β3 (datum construction out; tests to `crates/reify-stdlib/tests/`
through the public seams, subsuming #7779). The sixth `expr.rs` partition (the ladder) is
extracted by registry ω #6014 as `builtin_call_result_type` once the ladder is one registry arm.

**D18 — Approach B + H.** Blast radius ≥ 10 crates (reify-builtins, reify-core, reify-ast,
reify-syntax, reify-ir, reify-compiler, reify-expr, reify-stdlib, reify-eval, reify-lsp,
reify-audit, plus `gui/src-tauri`), mechanism count ≈ 17, consumers in S6, S12, S15, S23; the
builtin dispatch seam is load-bearing. Contracts §4; boundary tests §5.

## §4 — Contracts (H)

**C1 — Builtin outcome** (S8-α1; reify-core, reify-stdlib, reify-ir, reify-expr).
```text
reify_core::Rejection { code: DiagnosticCode, severity: Severity, message: String, arg: Option<usize> }
  Rejection::into_diagnostic(self, builtin: &str, span: SourceSpan) -> Diagnostic   // the only conversion
reify_core::Advisory  { diagnostic: Diagnostic, scope: AdvisoryScope }               // EveryCall | OncePerSink
impl From<reify_ir::arg_acceptance::ArgRejection> for Rejection                     // in reify-ir
reify_stdlib::Evaluated { value: Value, advisories: Vec<Advisory> }
  Evaluated::of(Value)                       // converted family; the settle point debug-asserts value != Undef
  Evaluated::with(Value, Vec<Advisory>)      // converted family with advisories
  Evaluated::legacy(Value)                   // unconverted family; Undef allowed
reify_stdlib::BuiltinOutcome = Result<Evaluated, Rejection>
reify_stdlib::eval_builtin(name: &str, args: &[Value]) -> BuiltinOutcome
```
Invariants: (i) a converted family reports failure only by `Err`; `Evaluated::of`/`with`
carrying `Undef` is a debug-assertion failure at the settle point. (ii) reify-expr has exactly
one settle function; for `Err(r)` it pushes the diagnostic, pushes `UndefCause::OpContractFailed
{ code: r.code, .. }` and returns `Undef`; for `legacy(Undef)` it keeps today's generic cause.
(iii) Advisories are pushed in the order returned; `OncePerSink` is honoured generically by the
settle point (the flexure fatigue Info is its first user; no family-specific rule lives there).
(iv) The settle function stays `#[inline(never)]` (the `eval_user_fn_recursion_depth_exceeded`
stack budget). (v) A pure function that computes a family's verdict diagnostics (dfm) is called
by the builtin to build its advisories and by reify-eval's `measure_dfm_rules` on the
constraint path — one producer, two callers, nothing re-derived.

**C2 — Call classification** (S8-ε, ζ; reify-builtins, reify-ir, reify-expr, reify-compiler).
```text
reify_builtins::{Intercept, InterceptKind, OperandGuard, UndefPolicy}          // row column, static
reify_builtins::{StdlibIntrinsic, STDLIB_INTRINSICS}                           // (name, arity) → intrinsic
reify_ir::CompiledExprKind::FunctionCall     { function, args, builtin: Option<BuiltinId> }    // registry β #6002
reify_ir::CompiledExprKind::UserFunctionCall { function_name, args, intrinsic: Option<StdlibIntrinsic> }
reify_expr::classify_call(id: BuiltinId, args: &[Value]) -> Option<InterceptKind>
reify_expr::classify_user_call(call_intrinsic: Option<StdlibIntrinsic>) -> Option<StdlibIntrinsic>
```
Invariants: `eval_expr` and `dual_eval` reach intercepts only through these two functions and
match their result exhaustively (no `_` arm); no bare-name dispatch on the call name remains in
either; `dual_eval`'s membership test iterates the rows' intercept column instead of a hand list.

**C3 — Datum projection** (S8-θ; reify-ir). Signatures in D7. Invariant: the compiler's typed
member set, `eval_datum_projection`'s handled set and reify-eval's self/feature sets are all
projections of the one table; a parity test iterates every `Resolved` row and asserts a
non-`Undef` value of the declared kind for a canonical receiver (the finding's proposal).

**C4 — Geometry result typing** (S8-η). `geometry_fn_result_type(name, argc) -> Type` total over
`GEOMETRY_FUNCTION_NAMES`; `is_kinematic_sweep(name, argc) -> bool` the only arity rule;
`E_GeometryCallInValuePosition` emitted by the compiler, never by eval.

**C5 — Field payload** (S8-ι). `FieldPayload` per D8; `Value::Field` has no `lambda` field and
no `source` field; `FieldPayload::kind() -> FieldSourceKind`.

**C6 — Tensor kernels** (S8-κ). `TensorKernel { kind: AnalysisKind, width: usize,
eval: fn(&[f64; 9]) -> Option<KernelOut>, validity: fn(&KernelArgs) -> Validity }`, one table;
both consumers iterate it; `AnalysisKind` matches exhaustively.

**C7 — Geometry dispatch** (S8-μ, ν1, ν2, ξ). `GeometryBuiltin::from_name`, satellites take
`GeometryOpDiscriminants`, `ModifyArgs` per D12.

**C8 — Syntax-boundary vocabularies** (S8-π, σ). `reify_ast::{BinOp, UnOp}` with `from_token`;
`reify_syntax::KEYWORDS` per D15.

**C9 — Adoption ratchet rows** (INV candidate 5; member `tests/infra/test_infra_adoption_ratchet.sh`,
manifest `tests/infra/adoption-ratchet.manifest`, row grammar in its header). Each extract leaf
adds or lowers its row in its own diff and names it in its done-criteria. The engine skips only
lines whose first non-blank character is `#`; Rust `//` lines **do** count, so each ERE must be
anchored on the code construct (e.g. `^\s*pub fn diagnose\(`), and each row's path is one
repo-root glob.

| row id | counts | path | creator | ceiling at creation → final |
|---|---|---|---|---|
| `stdlib-legacy-outcome` | `Evaluated::legacy(` wraps | `crates/reify-stdlib/src/**/*.rs` | S8-α1 | one per family (23) → lowered by α2, β1, γ, δ1, δ2 to the families §9 leaves `legacy` |
| `stdlib-post-hoc-diagnose-hook` | `pub fn diagnose(` / `pub fn flexure_diagnose(` definitions | `crates/reify-stdlib/src/**/*.rs` | S8-α2 | 7 (6 post-Undef + #8705's) → 0 at δ1 |
| `expr-emit-diagnostics-fn` | `fn emit_*_diagnostics(` definitions | `crates/reify-expr/src/**/*.rs` | S8-α2 | 1 → 0 at δ1 |
| `expr-name-guarded-intercept` | `match function.name.as_str()` dispatch | `crates/reify-expr/src/**/*.rs` | S8-ε | 0 |
| `dual-eval-intercept-mirror` | `fn reduction_kind_of(` / `fn field_reduction_kind(` / `fn is_intercepted_user_fn(` | `crates/reify-expr/src/dual_eval.rs` | S8-ε | 1 → 0 at S8-ζ |
| `compiler-user-shadow-check` | `functions.iter().any(\|f\| f.name ==` | `crates/reify-compiler/src/**/*.rs` | S8-ζ | 0 |
| `compiler-datum-member-table` | `const DATUM_PROJECTION_MEMBERS` | `crates/reify-compiler/src/**/*.rs` | S8-θ | 0 |
| `eval-datum-member-table` | `const SELF_DATUM_PROJECTION_MEMBERS` | `crates/reify-eval/src/**/*.rs` | S8-θ | 0 |
| `tensor-kernel-twin` | `fn project_*_sampled(` per-kind twins | `crates/reify-expr/src/field_reductions.rs` | S8-κ | 0 |
| `parameterized-type-twin` | `fn resolve_*_with_subst(` twins | `crates/reify-compiler/src/type_resolution.rs` | S8-ο | 0 |
| `ast-operator-string-match` | `op.as_str()` / `op == "` operator string tests | `crates/reify-compiler/src/**/*.rs` | S8-π | 0 |

## §5 — Boundary-test sketch (two-way)

| # | Scenario | Pre (main `a4a8862bcc`, recorded in the manifest) | Post | Leaf |
|---|---|---|---|---|
| 1 | `examples/process/std_process_dfm.ri` | its W/E/I DFM verdict diagnostics via `emit_dfm_diagnostics` and the constraint path | byte-identical, now advisories from one verdict function; `emit_dfm_diagnostics` gone | S8-α2 |
| 2 | `examples/flexures/yield_warning.ri` | `W_FLEXURE_YIELDING`, `W_FLEXURE_PRB_OUT_OF_RANGE`, one `W_FLEXURE_FATIGUE_CHECK_MISSING` Info | byte-identical; the Info deduped by `OncePerSink` | S8-α2 |
| 3 | CoM density fallback (fixture authored by S8-α2; no committed pin exists) | `SnapshotCenterOfMassDensityFallback` via `emit_snapshot_diagnostics` | same warning as an advisory | S8-α2 |
| 4 | `tests/prd-gate/fixtures/s8_geometry_builtin_silent_undef.ri` (`frame3`/`transform3`/`vec3`/`point3` wrong-kind args) | `undef` + only the generic OpContractViolation note | `undef` + one coded Error naming the builtin and argument | S8-β2 |
| 5 | `tests/prd-gate/fixtures/s8_sweep_malformed.ri` | 8 malformed calls `undef` + only the generic note | one coded Error each; controls unchanged | S8-δ2 |
| 6 | `max(f)` for a field `f`; `max(1, 2)`; a dual-eval solve over a model using `max(f)` | intercept by name; dual mirror by hand | one `classify_call`; primal identical on both paths; a new `InterceptKind` variant without arms fails to compile (recipe) | S8-ε |
| 7 | `tests/prd-gate/fixtures/s8_user_fn_shadows_builtin.ri` (user `clamp`, `floor`, `map_or/3`) | silent; user `map_or(1.0, 2.0, 3.0)` = `undef` (intercept) | `W_UserFnShadowsBuiltin` ×3; `m` = 222 (the user body) | S8-ζ |
| 8 | `tests/prd-gate/fixtures/s8_geometry_value_position.ri`; `…/s8_geometry_inline_overload_error.ri` | `pick(box(..))` silently picks `pick(x: Real)` → `undef`; `takes_geo(box(..))` → "no matching overload … (Real)"; `param pl : Length = box(..)` → errors naming `Real` | `E_GeometryCallInValuePosition` at both inline arguments; the `pl` errors name `Geometry`; `pick(b)` = 2 unchanged | S8-η |
| 9 | `tests/prd-gate/fixtures/s8_geometry_ctor_conformance.ri` (let-bound); `…/s8_geometry_ctor_conformance_inline.ri` | let-bound: silent, `Holder` fields hold geometry; inline: 2 errors naming `Real`, `origin` never rejected | let-bound: errors for `name`, `len` and `origin` naming `Geometry`; inline: `E_GeometryCallInValuePosition` | S8-η |
| 10 | `tests/prd-gate/fixtures/s8_geometry_annotations.ri`, `…/s8_geometry_arith_cmp.ri` | `pr`, `ll`, `li`, unannotated `mk` silent; `box*2`, `box+1`, `box>0` silent at check; `constraint box > 0` exit-0 INDETERMINATE | `ParamDefaultTypeMismatch` / `LetAnnotationTypeMismatch`; `FnReturnTypeUnannotated` naming `Geometry`; ArithOperandKind / CmpOperandKind errors (`box + 1` stays silent, §9) | S8-η |
| 11 | `examples/kinematic/counter_mass_balance.ri`; the 265-example corpus; `examples/half_space.ri` (#7505's widening control) | `snaps` typed `Real`; corpus diagnostics as measured | `snaps` typed `List<Snapshot>`, check + eval byte-identical; corpus byte-identical except `std_tolerancing_surface.ri`, whose inline constructor geometry is rebound with `let` (enumerated correction) | S8-η |
| 12 | `tests/prd-gate/fixtures/s8_frame_datum_planes.ri` | `i_yz`, `i_zx`, `g_yz`, `g_zx` = `undef`; every `f_*` (from `self.x`/`self.z`) = `undef` | all planes and members evaluate on identity, `fi.*`-built and `self.*`-built frames | S8-θ |
| 13 | `tests/prd-gate/fixtures/s8_safety_factor_nonpositive_yield.ri` | `sf_neg = -2.5`, `sf_zero = 0` | coded `SafetyFactorNonPositiveYield` Error + `undef` for both; `sf_pos = 2.5`; field `sample` and `max` agree | S8-κ |
| 14 | `examples/fea_multi_case_bracket.ri`; `solve_load_cases` nested in a constraint (dispatcher attached) | reify-expr copy runs; diagnostics duplicated across homes | `reify eval` output byte-identical; nested call returns the trampoline's result; `eval_solve_load_cases` absent | S8-λ |
| 15 | LSP rename of a binding to `match`, `forall`, `unit` | accepted | refused (Reserved); a Contextual word follows the parse test | S8-σ |
| 16 | `curvature(extremal_by_bbox(b)[0], p)` | `Scalar<Curvature>` | the face overload's matrix type | S8-ρ |
| 17 | Every leaf | — | workspace suite, examples_smoke and the prd-gate corpus green with unchanged expectations except the enumerated corrections in that leaf's text | all |

Row 17 is part of every leaf's done-criteria.

## §6 — Cross-PRD relationship (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Edges (A ← B: A depends on B) |
|---|---|---|---|---|
| `builtin-signature-registry.md` legs β..ω | this PRD owns | row shape §7.1 (amended here), legs adopted | S8 | §8 adopted table |
| `core-ast-ir-layering.md` §3 B3 + `scripts/assert-crate-dag.sh` | amended | reify-ir may depend on reify-builtins (D2) | core-ast-ir-layering; amended by registry β #6002's diff | #6002 details note |
| S7 compiler-module-namespace | consumes | `builtin_families.rs` (ε #8622; S8 owns later edits), explicit imports in `expr.rs`/`entity.rs` (θ #8627), five-of-six `expr.rs` partition (ι #8631), `operator_typing.rs` (δ #8620), `type_resolution` imports + `ResolutionCtx` (η #8625, μ #8643), `geometry_*` and `datum_projection.rs` imports (γ #8617, ζ #8623) | S7 produces | per leaf in §8 |
| S3 dependency-invalidation-stratum | consumes | `expr_classify.rs` name-set predicates and `FEATURE_DATUM_PROJECTION_MEMBERS` (β #8687) | S3 moves, S8 derives | S8-θ ← #8687; #6944 ← #8687 |
| S9 compiler-declaration-passes | consumes | `RelationSig` rows (λ #8741, held on S6) | S9 | #6944 ← #8741 |
| S6 kinematics | both | consumes `JointKind` (β #8829), `JointPosition` + `bind` contract (η #8834), sweep through `Joint` (ο #8843), dynamics on `MechanismTree` (ρ #8845), CoM group move (σ₀ #8846); produces S8-α1 for S6 χ #8851 | S6 / S8 | #6945 ← #8829; #6007 ← #8834; S8-δ2 ← #8843; S8-γ ← #8845; S8-α2 ← #8846; χ #8851 ← S8-α1 (lead releases) |
| S15 trampoline-decode | consumes | κ #8705's `analysis` hook and ξ #8717's `fea/` move become rejections; `SampledField::same_grid` in `value.rs`; `ArgRejection` converts into `Rejection` (C1) | S15 produces | S8-γ ← #8705, #8717; S8-ι ← #8717; S8-κ ← #8705; S8-λ ← #8717 |
| S13 selector-resolution-factoring | consumes | `geometry_ops/compile/` (δ #8750), `selector_eval/` (γ #8749) | S13 | S8-ξ ← #8750; #6944 ← #8749 |
| S12 check surface | produces | S8-α1's coded rejections; registry β #6002's `BuiltinId`; INV-SF-6 row is S12's | S8 | cross-reference |
| S23 prose rehoming | produces | row `basis` column holds the rationale prose S8 moves out of `builtin_arg_slots` and `geometry::diagnose`; S23 skips that prose and the `dual_eval.rs` line pointers S8-ε deletes | S8 | cross-reference |
| S24 reify-audit lanes | obligation | PLINECITE `--rename` for files S8 moves (stdlib `geometry.rs`, compiler `geometry.rs`, reify-expr `lib.rs`) once #8822 lands; `docs/invariants.md` Pointer update for INV-COMP-2 when registry λ #6015 flips it, once #8821 lands | S24 lane, S8 same-diff | same-diff obligation in S8-β3, ν2, τ, #6015 |
| S25 GUI backend | file lock | S8-ι edits the three `Value::Field` destructures in `gui/src-tauri/src/engine.rs`, which S25 partitions | S25 owns the file | cross-reference |
| S5 analysis stratum | file lock | S8-λ edits a comment in `compute_targets/multi_case.rs` and the multi-case tests; S5 moves compute targets | — | none |
| S4 cell-eval closure | file lock | S8-τ moves `EvalContext` into `reify-expr/src/context.rs` | — | none |
| #5425 geometry-position acceptance | ordering | edits `geometry.rs`, `geometry_traits_inference.rs`, `expr.rs`; S8-η changes the inline-call type it keys on | #5425 | none (file lock); details note on #5425 |
| #8480 dual_eval partition | file overlap | partitions `dual_eval.rs` (kink tables, `NonDifferentiable`, `Seeds`); only `is_bounded_reduction_shaped` overlaps S8-ε | #8480 | none (file lock) |
| #8558, #8549 (filed mechanicals) | referenced | expression-depth bound; Kleene left absorption | filed | none |

No new contested-ownership pair (checked against the overlay's three known pairs). The program
G4 row "reify-builtins row schema (compiler, eval, LSP, reify-audit RI_KEYWORDS all read it)"
is corrected by ruling Q2: reify-audit reads `reify_syntax::KEYWORDS`, not the row schema
(cross-reference for the lead).

## §7 — Pre-conditions

Registry β #6002 is the substrate for S8-ε, S8-ζ and every τ leg (it adds `BuiltinId` to
`CompiledExpr`); the S7 leaves precede the compiler leaves that edit their files (compiler
landing order S7, S9, S8, S12); S15 κ/ξ precede γ, ι, κ, λ; S3 β precedes θ; S13 δ precedes ξ;
S6 ρ/σ₀/ο precede γ, α2, δ2. No novel grammar: every fixture parses with
`tree-sitter parse --quiet` (manifest batch row); `grammar_confirmed=true` batch-wide (S8-π
changes the AST type, not the grammar).

## §8 — Decomposition plan

Every leaf carries §5 row 17. "Ratchet row" means C9's row created or lowered in that leaf's
diff. New gate-resident tests carry their drift-guard registrations in the same diff (overlay
rule). Prereqs use the `←` convention.

**Phase 1 — the typed outcome (foundation, then families).**

- **S8-α1 — Typed builtin outcome; one settle point; every caller (no behaviour change).**
  Modules: reify-core `diagnostics.rs` (`Rejection`, `Advisory`), reify-ir `arg_acceptance.rs`
  (`From<ArgRejection>`), reify-stdlib `lib.rs` + `registry_dispatch.rs` (`eval_builtin` returns
  `BuiltinOutcome`; the 23 families wrapped as `Evaluated::legacy`), reify-expr `lib.rs` (settle
  point), `analysis.rs`, `dual_eval.rs`, reify-eval `engine_constraints.rs`
  (`measure_gdt_conformance`), `geometry_ops.rs` (`fold_pose_chain`), `registry_parity_tests.rs`
  and the test modules that call `eval_builtin`, `tests/infra/adoption-ratchet.manifest` (row
  `stdlib-legacy-outcome`). Signal: §5 row 17 with zero diagnostic or value change across the
  examples corpus; ratchet line `stdlib-legacy-outcome` printed at its measured count (one per
  dispatch stage of `eval_builtin`). Consumer: S8-α2, β1, β2, γ, δ1, δ2,
  κ; S6 χ #8851; S12. Prereqs: none. Priority medium. Keys: none (foundation of
  fk-76e1dc7c434e).
- **S8-α2 — Success-path advisories: dfm, flexure, snapshot.** Modules: reify-stdlib `dfm.rs`
  (a pure verdict function the builtin and the constraint path both call), `flexures/`,
  `snapshot.rs`/`dynamics/com.rs` (CoM fallback), reify-expr `lib.rs`
  (`emit_{flexure,dfm,snapshot}_diagnostics` deleted), reify-eval `engine_constraints.rs`
  (`measure_dfm_rules` calls the verdict function), a new CoM-fallback fixture under
  `tests/prd-gate/fixtures/`, ratchet rows `stdlib-post-hoc-diagnose-hook`,
  `expr-emit-diagnostics-fn`. Signal: §5 rows 1–3 byte-identical. Consumer: δ1. Prereqs ← S8-α1,
  #8846. Keys: none.
- **S8-β1 — Geometry family: the 19 diagnosed names become rejections.** Modules: reify-stdlib
  `geometry.rs` (`diagnose`, `datum_fault_diagnostic`, `diagnose_bbox_corners` deleted; the
  rejecting arms return coded `Rejection`s built from the existing `DatumFault`/`AffineMapFault`
  classifiers); the ruling/escalation prose moves to the row `basis` or this PRD with a pointer.
  Signal: the 19 names' diagnostics byte-identical in message, now coded (`affine_scale` gains a
  code); ratchet `stdlib-post-hoc-diagnose-hook` −1, `stdlib-legacy-outcome` −1. Consumer:
  S8-β2. Prereqs ← S8-α1. Keys: none.
- **S8-β2 — Geometry family: the 28 undiagnosed names gain coded rejections.** Modules:
  reify-stdlib `geometry.rs`, reify-core `diagnostics.rs`. Signal: §5 row 4. Consumer: S8-β3,
  υ. Prereqs ← S8-β1. **Keys: fk-7e3c3d10ae46. Subsumes #6697.**
- **S8-β3 — Stdlib `geometry.rs` partition (3).** Modules: reify-stdlib `geometry.rs` → a
  datum-construction module + the eval module; the test module moves to
  `crates/reify-stdlib/tests/` through public seams. Signal: the moved tests green unchanged;
  `module_scc_ratchet` shows no new reify-stdlib row; §5 row 17. Prereqs ← S8-β2. Keys: none
  (ruled h14). **Subsumes #7779.** Same-diff: PLINECITE `--rename` once #8822 has landed.
- **S8-γ — FEA, dynamics and analysis rejections.** Modules: reify-stdlib `fea/combine.rs`
  (`linear_combine`'s ten checks become its rejections; `fea::diagnose` deleted),
  `dynamics/eval.rs` (`inverse_dynamics_*`; the "best-effort attribution" goes), `analysis.rs`
  (#8705's `StressTensorAsymmetric` hook becomes a rejection). Signal: `reify eval` on the
  existing multi-load and dynamics fixtures that exercise those diagnostics — byte-identical
  messages, now coded at the rejecting arm; ratchet `stdlib-post-hoc-diagnose-hook` −3.
  Consumer: δ1. Prereqs ← S8-α1, #8705, #8717, #8845. **Keys: fk-eca29487b2c9.**
- **S8-δ1 — Stackup, tolerancing, orientation rejections; the post-hoc axis deleted.** Modules:
  reify-stdlib `stackup.rs`, `tolerancing.rs` (its two code-less Errors gain codes),
  `orientation.rs` (`orient_exp`), `lib.rs` (the `pub use … diagnose` re-exports), reify-expr
  `lib.rs` (`emit_undef_builtin_diagnostics` deleted). Signal: the three families' diagnostics
  byte-identical in message, now coded; ratchet `stdlib-post-hoc-diagnose-hook 0`,
  `expr-emit-diagnostics-fn 0`. Consumer: δ2, τ. Prereqs ← S8-α2, β1, γ. Keys: none.
- **S8-δ2 — Sweep rejections.** Modules: reify-stdlib `sweep.rs` (the remaining silent exits
  return coded rejections), reify-core `diagnostics.rs`. Signal: §5 row 5; ratchet
  `stdlib-legacy-outcome` −1. Consumer: υ. Prereqs ← S8-δ1, #8843. **Keys: fk-76e1dc7c434e.**

**Phase 2 — one classifier.**

- **S8-ε — Intercept column and `classify_call` (high).** Modules: reify-builtins `row.rs`,
  `registry.rs` (`Intercept` column; `ExprIntercept` rows for the unconditionally intercepted
  names; `EvalBuiltin` rows carrying the column for the conditional ones — `fn_field`,
  `restrict`, the analysis names, `max`/`min`/`argmax`/`argmin` at both arities — with result
  specs reproducing today's compiler typing; the numeric `max`/`min`/`argmax`/`argmin` rows ship
  with `Any` slots and `basis: Artifact`, upgraded by #6943; carved out of #6944's field-op rows
  and #6943's numeric rows), reify-stdlib `registry_dispatch.rs` (their eval arms move under the
  exhaustive match), reify-expr `Cargo.toml` (+reify-builtins), `lib.rs` (`classify_call`; the
  `FunctionCall` arm matches its result), `dual_eval.rs` (calls it; `reduction_kind_of`,
  `field_reduction_kind` and the 21 `lib.rs:NNN` pointers deleted; the membership test iterates
  rows), `crates/reify-builtins/tests/i_reg_1_seed_string_dispatch_gate.rs` (ledger entries
  retired). Signal: §5 row 6 — a dual-eval solve over a model using `max(f)` gives the same
  primal as `eval_expr`; adding an `InterceptKind` variant without arms fails to compile
  (documented recipe); ratchets `expr-name-guarded-intercept 0`, `dual-eval-intercept-mirror 1`.
  Consumer: ζ, τ. Prereqs ← #6002. Priority high (gate #8612). **Keys: fk-be87290e126e.**
- **S8-ζ — Stdlib intrinsics stamped at compile time; one shadowing rule.** Modules:
  reify-builtins (`STDLIB_INTRINSICS`; rows for `filter` — `EnginePostProcess`, whose
  `structural_query.rs` name compare becomes an id match — and `cost` — `CompileOnly`, its
  compile-time rewrite), reify-ir `expr.rs` (`intrinsic` on `UserFunctionCall`), reify-compiler
  `expr.rs` (stamp from static resolution; `filter`/`cost`/`generate` after user-fn resolution
  keyed by `BuiltinId`; `W_UserFnShadowsBuiltin`), reify-core `diagnostics.rs`, reify-expr
  `lib.rs` (`classify_user_call`; the stamp wins over runtime re-resolution), `dual_eval.rs`
  (`is_intercepted_user_fn` deleted), reify-eval `structural_query.rs`. Signal: §5 row 7;
  ratchets `compiler-user-shadow-check 0`, `dual-eval-intercept-mirror 0`. Consumer: τ.
  Prereqs ← S8-ε, S8-λ, #6002, #8627, #8631. Keys: none (contributes to fk-be87290e126e and
  fk-935b6eca2b50). **Subsumes #8578.**

**Phase 3 — compiler typing and dispatch.**

- **S8-η — Geometry calls typed by kind; lowering-unreachable geometry calls are a coded error
  (high).** Modules: reify-compiler `builtin_families.rs` (`geometry_fn_result_type`,
  `is_kinematic_sweep`), `expr.rs` (ladder arm 9; `E_GeometryCallInValuePosition`),
  `geometry.rs`, `conformance/mod.rs` (`arg_type_is_unverifiable` narrowed; 3 name sniffs
  deleted), `geometry_traits_inference.rs` (disjunct deleted; `List<Real>` test pins
  rewritten), `entity.rs` (overrides → `debug_assert_eq!`; geometry-let annotation check),
  `struct_ctor_field_conformance_tests.rs` (its placeholder "ruling" pin), reify-core
  `diagnostics.rs`, `examples/tolerancing/std_tolerancing_surface.ri` (enumerated correction).
  First step: probe whether realization lowering reaches `sub` geometry arguments (D6 table) and
  record the result in the task. The text enumerates #7505's `E_GEOMETRY_UNBOUNDED` widening on
  nested unbounded operands with `examples/half_space.ri` as the control. Signal: §5 rows 8–11.
  Consumer: #6944 (τ9 rows), #6007 (τ5 sweep typing), S8-μ, φ, υ, #6014. Prereqs ← #8622,
  #8627, #8631. Priority high (gate #8612). **Keys: fk-5a6c2aff4589.**
- **S8-μ — Geometry lowering keyed by the descriptor discriminant.** Modules: reify-compiler
  `geometry.rs` (`GeometryBuiltin::from_name`; router), `geometry_boolean.rs`,
  `geometry_curve.rs`, `geometry_transform.rs`, `geometry_modify.rs` (exhaustive matches; the
  four `unreachable!` and the inner `name → BooleanOp` match deleted). Signal: an alias or op
  added to a descriptor without a satellite arm fails to compile (recipe in the task);
  `git grep 'unreachable!(' -- crates/reify-compiler/src/geometry_{boolean,curve,transform,modify}.rs`
  empty; §5 row 17. Consumer: ν1, ξ. Prereqs ← #8617, #8623, S8-η. **Keys: fk-2e03b9202f6d.**
- **S8-ν1 — Arg indices, profile preconditions and trait-inference arms from the descriptor.**
  Modules: reify-compiler `geometry.rs` (`geometry_arg_indices`, `check_profile_preconditions`
  read `parent_role` + the composite table; the silent default goes),
  `geometry_traits_inference.rs` (per-op arms keyed by `GeometryBuiltin`). Signal: the
  cross-check tests reduce to one descriptor-coverage test; a descriptor row without an
  arg-index disposition fails to compile; §5 row 17. Consumer: ν2. Prereqs ← S8-μ. Keys: none.
- **S8-ν2 — Compiler `geometry.rs` partition (7).** Modules: reify-compiler `geometry.rs` → the
  seven h14 groups; `compile_geometry_call_inner` becomes one exhaustive `match GeometryBuiltin`
  whose arms call per-family lowering functions. Signal: each new file states its purpose in
  one sentence; no new reify-compiler SCC row once S7 ξ #8657 has added the compiler rows; §5
  row 17. Consumer: #6944. Prereqs ← S8-ν1. **Keys: fk-ea33d4b5d69d.** Same-diff: PLINECITE
  `--rename` once #8822 has landed.
- **S8-ξ — Typed Modify arguments.** Modules: reify-compiler `types.rs` (`ModifyArgs`),
  `geometry_modify.rs`, reify-eval `geometry_ops/compile/` modify family, `deps.rs`,
  `engine_build.rs`. Signal: no string-keyed lookup of a Modify argument remains in reify-eval
  (`git grep -E 'n == "(open_faces|thickness|third|radius|distance)"' -- crates/reify-eval/src`
  empty); a misspelt field fails to compile; §5 row 17 on the modify examples. Consumer:
  reify-eval modify compilers. Prereqs ← S8-μ, #8750, #8623. **Keys: fk-a494d2285710.**
- **S8-ο — One parameterized-builtin-type table; one resolver per result kind.** Modules:
  reify-compiler `type_resolution.rs`. Signal: ratchet `parameterized-type-twin 0`;
  `param v : Vector3<L>` through a generic alias (the subst path) classifies its dimension slot
  as the base path does (the task enumerates the behaviour change); §5 row 17. Consumer: #7736
  (may read the table). Prereqs ← #8625, #8643. **Keys: fk-d402f7243603.**
- **S8-π — Operators parsed at the syntax boundary.** Modules: reify-ast `ast.rs`
  (`BinOp`/`UnOp` moved in), reify-ir (re-export), reify-syntax `ts_parser.rs`, reify-compiler
  `operator_typing.rs`, `type_compat.rs` remnants, `type_resolution.rs`, `geometry.rs`,
  `expr.rs`, and every other `ExprKind::BinOp` matcher (reify-lsp, reify-test-support,
  reify-constraints, reify-eval, `gui/src-tauri` — the task lists them from `git grep`).
  Signal: ratchet `ast-operator-string-match 0`; `git grep 'fn resolve_binop'` empty; §5 row 17.
  Consumer: every operator consumer. Prereqs ← #8620. **Keys: fk-e187a67f4b32.**
- **S8-ρ — Curvature face overload by type.** Modules: reify-compiler `builtin_families.rs`.
  Signal: §5 row 16 (`reify check` on a fixture authored in the task). Prereqs ← #8622.
  Priority low. **Keys: fk-eeebffcc4a83.**

**Phase 4 — satellite tables.**

- **S8-θ — One datum-projection table in reify-ir (high).** Modules: reify-ir new
  `datum_projection.rs`, reify-compiler `datum_projection.rs` (reads the table), reify-expr
  `lib.rs` (`eval_datum_projection`), reify-eval `geometry_ops.rs` self-datum path and S3's
  `expr_classify.rs` feature set. Signal: §5 row 12; the C3 parity test; ratchets
  `compiler-datum-member-table 0`, `eval-datum-member-table 0`. Consumer: compiler typing, eval,
  reify-eval, τ. Prereqs ← #8687, #8617 (S7 γ rewrites `datum_projection.rs`'s imports).
  Priority high (gate #8612). **Keys: fk-7bfe957c7f09. Subsumes #6100** (GAP 1, GAP 2 on
  `self`-built frames, item 4's lockstep pin).
- **S8-ι — `FieldPayload` typed variants.** Modules: reify-ir `value.rs`, reify-expr `lib.rs`
  (`sample_field_at`, `sample_as_printed_zones`), `analysis.rs`, `calculus.rs`,
  `field_reductions.rs`, `dual_eval.rs`, reify-eval `compute_targets/{mod,elastic_static,
  as_printed_material,as_printed_material_r0}.rs`, `engine_eval.rs`, `engine_build.rs`,
  reify-stdlib `fea/`, `gui/src-tauri/src/engine.rs` (three destructures), reify-test-support
  `builders/expr.rs`, and every test file that builds a `Value::Field` literal (the task lists
  them from `git grep -l FieldSourceKind`). Signal: `git grep 'items.len() == 7'` empty in
  reify-expr; the as-printed and field-sampling examples byte-identical; the `Imported` doc row
  matches the type. Consumer: κ, τ. Prereqs ← #8717. **Keys: fk-561a5a490532.**
- **S8-κ — One tensor-kernel table; non-positive yield rejected.** Modules: reify-expr
  `analysis.rs`, `field_reductions.rs`, reify-stdlib `analysis.rs` (`safety_factor`
  rejection), reify-core `diagnostics.rs`. Signal: §5 row 13; ratchet `tensor-kernel-twin 0`;
  `every_wrapper_kind_samples_to_its_reduction_extrema_at_their_arg_coordinates` stays green.
  Consumer: the next analysis kind, τ. Prereqs ← S8-ι, S8-α1, #8705. **Keys: fk-ecc0b4fb9d6b.
  Subsumes #7847.**
- **S8-λ — One `solve_load_cases` home.** Modules: reify-expr `lib.rs`, `dual_eval.rs`,
  reify-stdlib `fea/` stub arm, reify-eval `compute_targets/multi_case.rs` (the "mirrors"
  prose), the task-3005 smoke tests (rewritten onto a production-registered engine). The text
  enumerates both D10 behaviour changes and the constraint-path solve cost. Signal: §5 row 14;
  `git grep -E 'fn (eval_solve_load_cases|invoke_solve_elastic_static)'` empty. Consumer: ζ, τ.
  Prereqs ← #8717. **Keys: fk-a72f4510f36b.**
- **S8-σ — One keyword table in reify-syntax.** Modules: reify-syntax (new `keywords.rs` +
  parse test + spec-parity test), reify-lsp `references.rs`, `completion.rs`, reify-audit
  `pdoccover.rs` and `Cargo.toml`. Signal: §5 row 15 (rename refused, observed through the LSP
  rename harness on a real document); `reify-audit --pattern PDOCCOVER` output unchanged.
  Prereqs: none. **Keys: fk-81f949615552. Subsumes #8583.**

**Phase 5 — partition, docs, milestone, close.**

- **S8-τ — reify-expr `lib.rs` partition (8).** Modules: reify-expr `lib.rs` → `context.rs`,
  `value_ops.rs`, `methods.rs`, `overload.rs`, `intercepts.rs`, `sampling.rs`, the core
  evaluator in `lib.rs`, tests to `crates/reify-expr/tests/` through public seams. Signal: no
  reify-expr row in `scripts/module-scc-baseline.json` (`module_scc_ratchet` green); each new
  file states its purpose in one sentence; §5 row 17. Prereqs ← S8-α2, δ1, ε, ζ, θ, ι, κ, λ.
  Keys: none (ruled h14). Same-diff: PLINECITE `--rename` once #8822 has landed.
- **S8-υ — Docs-truth.** Modules: `crates/reify-mcp/src/tools/chunks/*.md` (geometry values and
  the bind-with-`let` rule; `W_UserFnShadowsBuiltin`; the new geometry-builtin and sweep
  rejection codes; `safety_factor`'s yield rule; Frame plane projections),
  `examples/best_practices/` exemplar "bind geometry with `let`, then pass it" + `INDEX.md` line,
  `.claude/skills/reify-design/SKILL.md` index line. Signal: every documented signature compiles
  as written in a smoke `.ri`; an author searching for "pass a shape to a function" finds the
  exemplar from the chunk index. Prereqs ← S8-η, ζ, κ, θ, σ, β2, δ2.
- **S8-φ — [MILESTONE] PRD: hoist value-position geometry calls.** Not an implementation leaf
  (`execution_class: decision`). On dispatch it escalates for a human `/prd` session to author
  the hoisting PRD that replaces `E_GeometryCallInValuePosition` with evaluation and covers the
  unchanged positions in D6's table (function and lambda bodies) together with #5428 (session
  ruling Q4). Prereqs ← S8-η.
- **S8-ω — PRD close.** Stamps this PRD's Status `SHIPPED` with the landed leaf ids, the
  AS-AUTHORED freeze paragraph and the LIVE/AS-AUTHORED map, the same header on the
  `.capability-manifest.md`, and the registry PRD's own terminal stamp. Signal: the committed
  headers. Prereqs ← every leaf above except S8-φ, plus registry λ #6015 and ρ #6016.

**Adopted registry legs** (filed; stamped and wired at decomposition, never refiled):

| Task | Leg | Action at decomposition |
|---|---|---|
| #6002 | β: `BuiltinId` in `CompiledExpr` | details note: D2 (inline; reify-ir and reify-expr gain reify-builtins; `assert-crate-dag.sh` B3 + `core-ast-ir-layering.md` §3 amended in the same diff); prereq of S8-ε, ζ |
| #6943 | τ1+τ6 numeric + complex | details note: `max`/`min`/`argmax`/`argmin` rows are S8-ε's, #6943 upgrades their slots; #6943 ← S8-ε |
| #6944 | τ7+τ8+τ9 | stamp **fk-ffa4ba4c2d0b** (done adds: `builtin_arg_slots` deleted, its rationale in row `basis`), **fk-07c78c4ca710** and **fk-c02915c1952e** (done adds: one `TopologySelectorHelper` dispatch keyed by `BuiltinId`, arity from the row); details note: field-op intercept rows are S8-ε's, `filter`/`cost` rows are S8-ζ's, GeometryOp rows carry S8-η's result types (§4 exclusion struck), the descriptor keys names by `BuiltinId` and `GeometryBuiltin::from_name` goes; #6944 ← S8-ε, ζ, η, ν2, #6943, #8687, #8741, #8749 |
| #6945 | τ3 joints | #6945 ← #8829 (`JointKind::from_ctor_name`) |
| #6007 | τ5 mechanism/snapshot/sweep | details note: absorbs S8-η's `is_kinematic_sweep` typing; the `bind` row follows S6 C4; #6007 ← S8-η, #8834 |
| #6012 | π LSP | stamp **fk-419ebb996480**; details note: hover renders builtin-call signatures from rows too |
| #6014 | ω fallback deletion | stamp **fk-33801a7c7a36**, **fk-935b6eca2b50**, **fk-d50200501a51**; done adds: ladder is one registry arm extracted as `builtin_call_result_type` (the sixth `expr.rs` partition), the 30 `*_NAMES`, `BUILTIN_NAME_FAMILIES` and the pairwise test deleted, the precedence comment gone; #6014 ← S8-η, ζ, #8631 |
| #6015 | λ integration gate | INV-COMP-2 → enforced(type) (existing); same-diff Pointer update once #8821 has landed |
| #6004, #6006, #6016 | τ2, τ4, ρ | adopted unchanged |

**Filed mechanicals** (referenced, not refiled): #8558 carries fk-4bd71b633033; #8549 carries
fk-2845dd9262b4.

**Subsumed at decomposition** (cancelled with a reason naming the subsuming leaf, content ported
first): #6697 ← S8-β2, #7779 ← S8-β3, #8578 ← S8-ζ, #6100 ← S8-θ, #7847 ← S8-κ, #8583 ← S8-σ.
Re-checked at pass 2 with `get_task`; #7505 (trait inference skips placeholder-typed nested
geometry calls) is a candidate for subsumption by S8-η, confirmed or left at pass 2. Details
note on #5425: S8-η changes the inline geometry-call type its acceptance keys on.

Gate #8612 ← S8-ε, S8-η, S8-θ.

## §9 — Out of scope

- Hoisting value-position geometry calls into evaluation, and geometry calls in function and
  lambda bodies (S8-φ milestone; #5428; session ruling Q4).
- `Geometry + Int` silence (`infer_binop_type`'s `Add`/`Sub` lack an operand-kind guard) —
  follow-up candidate, not filed; INV-COMP-3 territory.
- Converting the stdlib families outside this PRD's findings from `Evaluated::legacy` to typed
  rejections (joints, numeric, complex, matrix, construct, loads, supports, mechanism,
  tensegrity, trajectory, list, trig, linalg) — follow-up candidate; the
  `stdlib-legacy-outcome` ratchet keeps the count visible.
- Typed argument records for the other string-keyed `CompiledGeometryOp` variants (Transform,
  Pattern, Sweep, Curve, Profile, Surface, Primitive) — follow-up candidate after S8-ξ proves
  the shape.
- The `.ri` stub bodies of `@optimized` functions evaluated without a dispatcher (INV-SF-5's
  general question; D10 only states the rule).
- The expression-depth bound (#8558) and Kleene left absorption (#8549): filed, owned there.
- The user-shadows-stdlib name environment (stdlib-namespace #5493/#5503): S8-ζ's warning covers
  builtins and stdlib intrinsics only; `.ri`-declared stdlib names stay #5503's.

## §10 — Open questions (tactical)

1. **`Rejection.arg` granularity** — argument index only, or index plus a slot name once rows
   carry named slots. Suggested: index now; S8-α1 decides.
2. **`InterceptKind` variant grain** — one variant per family with a payload
   (`FieldReduction(kind, extent)`) or flat. Suggested: per family. S8-ε decides.
3. **`W_UserFnShadowsBuiltin` severity** — Warning per #8578; revisit if #5503 escalates
   user-shadows-stdlib to Error. S8-ζ records its choice.
4. **Where `FieldPayload::Derived` keeps its op** — one `DerivedOp` enum or one variant per op.
   S8-ι decides.
5. **Naming the optional third Modify arguments** (`offset_curve`'s `third`) — name the field
   after its meaning, read from the stdlib signature. S8-ξ decides.

## §11 — Session rulings (Leo, 2026-10-13)

- **Q1 diagnose half of the column** → typed outcome: `eval_builtin` returns
  `Result<Evaluated{value, advisories}, Rejection>`; rows carry only the intercept column; the
  hooks go family by family; S6 χ and S15 κ/ξ land on the outcome (D4).
- **Q2 reserved-keyword home** → reify-syntax `KEYWORDS` with a parse-behaviour test; reify-audit
  gains a reify-syntax dependency (D15).
- **Q3 `safety_factor` with yield ≤ 0** → rejected on every path (D9).
- **Q4 value-position geometry calls** → "Eventually: hoist inline geometry args. If it doesn't
  fit gracefully here then coded error, bind with let + a PRD milestone for the hoist." It does
  not fit gracefully in S8-η (a realization-lowering feature inside a type-hygiene leaf): S8-η
  ships `E_GeometryCallInValuePosition` at the argument positions of D6's table; S8-φ is the
  milestone (D6).

Evidence behind the rulings is the session's seat records (geo-type, rt-intercept, rt-values,
cmp-map, inventory, fixtures, critic), summarised in §2 and the capability manifest.

## §12 — Gate walk (author mode)

- **G1**: every mechanism in §1's table names a consumer in this batch or a filed task.
- **G2**: every leaf in §8 names a `reify check`/`reify eval`/LSP/ratchet signal; intermediates
  (S8-α1, β1, μ, ν1, ε) name the leaves they unlock.
- **G3**: no novel grammar; `BuiltinId` in `CompiledExpr` is registry β #6002's deliverable,
  wired upstream of S8-ε/ζ; `try_compute_dispatch` and `ComputeDispatch` exist on main (S8-λ);
  reify-ir is a common dependency (S8-θ); the adoption-ratchet member exists (C9); the D5 stamp's
  provenance exists at resolution (`merge_prelude_functions`).
- **G4**: §6; no reciprocal ownership; B3's owner named.
- **G5**: B + H (D18).
- **G6**: §5's Pre column is recorded at main from committed fixtures and examples (manifest);
  S8-η's corpus claim measured over 265 examples, with its one correction enumerated; no
  numeric bound asserted.
- **G7** (advisory): INV-SF-1 strengthened (S8-α1: specific `UndefCause` codes); INV-SF-5 (S8-η
  retires a misstated placeholder type); every `Rejection` coded (INV-SF-6, S12's row); no waiver.
- **META**: yes — the remaining choices in §10 are local to one leaf each.
