# Type hygiene: relational/logical operand guards, polymorphic zero, ONE density contract, uniform builtin-arg acceptance

**Milestone:** v0_6 · **Status:** active · **Date:** 2026-06-10 · **Approach: B+H** (contracts + two-way boundary tests — blast radius spans reify-compiler / reify-eval / reify-expr / reify-ir / reify-core + stdlib/examples; trait-conformance machinery touched)

Authored from the 2026-06-10 type-hygiene deep-dive (spawn brief `~/.claude/spawn-briefs/type-hygiene-prd-2026-06-10.md`; primary evidence `docs/design/rigid-moment-of-inertia-shape-2026-06-10.md` §2/§6.3, probes preserved at `/tmp/moi-probe{1..8}.ri`). All file:line citations re-verified 2026-06-10 by survey agents against the working tree.

## 1. Goal

A Reify author can no longer write a constraint or builtin call that *silently never works*. Concretely, after this PRD:

- `constraint <tensor> > <scalar>` is a **compile error with a fixit** (suggesting `eigenvalues(t)[0] > 0` / `trace(t) > 0`), not a permanently-INDETERMINATE constraint.
- `constraint mass > 0` **works** — a literal zero adopts the dimension of the other operand (comparisons *and* additive positions), deleting the `0.0 * 1kg * 1m * 1m` stdlib boilerplate.
- `moment_of_inertia(b, material.density)`'s density arg follows **one** contract shared with `body_mass_props`: dimensioned `Density` accepted, everything else **loudly** rejected with a migration hint — and a `Pressure` passed as density is no longer silently treated as kg/m³.
- A recognized builtin given an argument it can't use emits a **diagnostic naming the builtin, the argument, the expected type, and what it got** — the "unsupported arg shape → silent Undef" contract is dead.
- A conformer member colliding with a trait member name **type-checks against the trait's declared type** — defaulted or not, param or let.
- `reify check --strict` exits non-zero when any constraint is INDETERMINATE, so CI can gate on data indeterminacy.

## 2. Background

Three issue clusters with one root: definedness failures are silent at every layer.

**Cluster 1 — no operand checking on relational/logical ops.** `infer_binop_type` returns `Type::Bool` unconditionally for `Eq/Ne/Lt/Le/Gt/Ge/And/Or/Implies` (`crates/reify-compiler/src/type_compat.rs:857-905`, arms at `:863-872`). The expr-compile check site at `crates/reify-compiler/src/expr.rs:1088-1191` already hosts four operand guards — Modulo→Int (`:1088-1108`, task 3916), Add/Sub dimension (`:1110-1149`), Pow (task 3805), Implies→Bool (`:1160-1191`, task 3921) — with full `Type` info on both compiled operands (`compiled_left/right.result_type`). Comparisons and And/Or are simply missing arms. At runtime `eval_cmp` (`crates/reify-expr/src/lib.rs:3635-3668`) Undefs three whole families — any tensor operand (via `as_f64` → `None`, `crates/reify-ir/src/value.rs:1121-1128`), any Enum operand, dimensioned-vs-non-Scalar — and the constraint checker then misdiagnoses "indeterminate: undefined inputs" (`crates/reify-constraints/src/lib.rs:75-86`) even when every input is defined. This is the esc-3115-112 family; task 4226 institutionalized the dimensioned-zero RHS boilerplate as a convention-level workaround.

**Cluster 2 — two divergent density contracts.** Contract A (`resolve_density_arg`, `crates/reify-eval/src/geometry_ops.rs:3961-3987` + `resolve_real_scalar_arg` `:3996-4012`; used by `moment_of_inertia`/`center_of_mass`, dispatch `:3163-3186`): accepts ONLY a `ValueRef` to a bare `Real`/dimensionless; dimensioned `Density` (= what `material.density` resolves to, `Material.density : Density` since task 3111, `crates/reify-compiler/stdlib/materials_mechanical.ri:73-77`) → Warning + Undef; inline literal / field access → **silent** Undef (documented contract at `:3970-3974`). Contract B (`resolve_body_density`, `crates/reify-eval/src/dynamics_ops.rs:91-110`; used by `body_mass_props`): explicit arg → body `Material.density` → default-water + `W_DynamicsDefaultDensity`. Survey found two *additional* B-side gaps: `cell_f64` (`:46-53`) extracts `si_value` from **any** dimensioned scalar verbatim (a `Pressure` passed as density is silently treated as kg/m³), and an explicit density arg in an unsupported expr shape is silently **ignored** (`resolve_arg_value` `:197-206` returns `None` → ladder falls through to Material/water — the user's explicit arg discarded without a diagnostic).

**Cluster 3 — per-builtin shape-matching instead of value-level acceptance.** 14 resolver fns in `geometry_ops.rs` pattern-match `CompiledExprKind` shapes (table in §8.2) serving ~37 geometry/dynamics builtins; all but one fall through **silently** on unsupported shapes. The 30 math builtins ride the clean `eval_expr` path but have result-type-only compiler signatures (`math_signatures.rs`, task 4182 — no per-arg slots). Point-fix history proving this is systemic: 1609 → 1715 ("the fix should be generic, not per-function copy-paste") → 2494 → unified-DAG red-team breaker (`resolve_geometry_handle_arg`) → `resolve_density_arg` (found 2026-06-10).

**Cluster 4 — conformance blind spots.** Conformer redeclarations are type-checked only against **required** (no-default) trait params (`conformance/checker.rs:1464-1492`, check at `:1482`; defaulted params never enter the requirements list). Trait `let`s are injected only-if-absent (`:1561-1844`, shadow check `:1695`) — a colliding conformer member silently shadows with **no type check** (probe 7: scalar override silently accepted against a tensor-defaulted trait param).

## 3. Resolved design decisions (Leo, 2026-06-10 session)

1. **Order comparisons on non-scalar operands = compile-time type error** (not numpy-style broadcast). Rationale: elementwise>0 ≠ positive-definiteness for the motivating inertia-tensor case; a constraint needs one Bool; numpy itself refuses `bool(array)`. Fixit suggests existing dimension-preserving reductions (`eigenvalues(t)[0]`, sorted ascending — `math_signatures.rs:265,:273`, `reify-stdlib/src/matrix.rs:223`; `trace(t)`).
2. **Dimension mismatch in scalar comparisons = compile error** — same guard arm covers kind- and dimension-mismatch.
3. **`==`/`!=` error uniformly with the order ops.** Structural tensor equality has no named consumer (G1) and FP equality on computed tensors is a footgun. Revisitable later as a non-breaking widening.
4. **Polymorphic literal zero EVERYWHERE** (comparisons + additive positions): a syntactic literal `0`/`0.0` (incl. unary-negated) whose sibling operand is `Scalar<D>` coerces to `Scalar<D>(0)` at **compile time** (literal rewrite; value layer untouched — no interaction with 4374's canonicalization). Scalar dimensions only — no adoption against tensor operands (those are guard errors). Not constant-folded zeros (`1-1` does not coerce; syntactic only). Deliberately re-litigates esc-3115-112 for ergonomics; the 4226 boilerplate gets *deleted* in a migration sweep.
5. **And/Or gain the compile-time Bool operand guard** (uniform with Implies/3921; re-opens the 3921-era scope note at `expr.rs:1158-1159` — confirmed a scope-bounding decision, not a semantic commitment). **Kleene three-valued runtime semantics over Bool-typed Undef are preserved verbatim** (`eval_and`/`eval_or`, `reify-expr/src/lib.rs:2507-2545`: `false ∧ Undef = false`, `true ∨ Undef = true`, short-circuiting). The guard is static-type-only; nothing working can break (non-Bool operands never produced a defined value — `KBool::try_from` accepts only Bool/Undef). False-positive surface checked: predicates are statically Bool (`is_on` `units.rs:234`, `contains`/`intersects`/`geo_equiv` `:551-553`), determinacy intrinsics compiler-typed, comparisons Bool by construction.
6. **Guard gradualism:** guards fire only on *definitely known* mismatches. `Type::Error` (poison) and `TypeParam`/unresolved-auto operand types pass through silently — constraint-aware resolution is the auto-type-param batch's territory (4431-4438).
7. **ONE density contract.** Accepted value = `Value::Scalar{MASS_DENSITY}` (`reify-core/src/dimension.rs:198`; si_value already kg/m³) via a **shared acceptance helper** used by both Contract A and Contract B. Bare Real → migration diagnostic naming `7850kg/m^3`. Wrong dimension → loud error (kills the Pressure-as-density hole). Explicit-arg-ignored hole killed. The "Material rung for geometry helpers" question dissolves: `moment_of_inertia(solid, density)` takes a bare solid, which carries no Material — explicit Density is the right contract there; the ladder belongs to body-shaped callers.
8. **Task 4473 is SUPERSEDED into this batch** (was: the narrow Contract-A fix). Its full scope is task γ below; 4229's dependency is repointed 4473→γ at decompose; 4473 cancelled-superseded.
9. **Default-water rung: keep warn+water in the INTERIM.** `W_DynamicsDefaultDensity` is not silent; exploratory flows keep working. The flip to hard-error-with-hint is owned by the companion **ambient-default-material PRD** (`docs/prds/v0_6/ambient-default-material.md`), which replaces the rung with a scoped `default Material = …` mechanism. No NEW surface ever gets a density default.
10. **Uniform arg acceptance = evaluate-then-accept.** Recognized-builtin args are evaluated to `Value`s (the `eval_named_arg` pattern, `geometry_ops.rs:~157-220`, call site `:535`, proves this works against the ValueMap at dispatch time) and pass ONE value-level acceptance check per declared arg type. Distinguish **undefined value** (data indeterminacy → quiet Undef propagation, existing degradation contract) from **unsupported kind/dimension** (→ loud diagnostic, always). The two geometry-handle resolvers are excluded — owned by unified-DAG ε=4358 (§6).
11. **Compile-time per-arg signatures** for builtin families (extends the 4182 result-type precedent with arg slots), firing call-site diagnostics on definite static mismatches only (per decision 6). `density: Real` doc-comments (`units.rs:158-159`) become enforced `density: Density`.
12. **Conformance collision rule:** any conformer member colliding with a trait member name type-checks against the trait's declared type — defaulted or not, param or let — via `implicitly_converts_to`. Type-**compatible** collision remains legal (it is the override idiom; a conformer may override a derived trait-let with a measured value). Implementation site: new loop over `ctx.defaults` after the required-members loop in checker phase 5.
13. **Ride-alongs in scope:** `reify check --strict` (fails on INDETERMINATE; today `reify-cli/src/main.rs:539,656,831` exit 0); honest-indeterminacy diagnostics (constraint reporting distinguishes "operator undefined for these operands" from "undefined inputs"); `MassProperties.inertia` dimension tightening `Matrix<3,3,Real>` → `Matrix<3,3,MomentOfInertia>` (`stdlib/dynamics.ri:71-83`).
14. **Sequencing vs real-dimensionless batch:** only the guard/zero leaves (α, β) depend on α4373 (`Type::Real` deletion) so they match a single canonical dimensionless type. The rest of the batch is independent. No scope duplication with 4372-4377.

## 4. Out of scope (named)

- **Full Undef taxonomy** (deferred/failed/never; "provably never-defined cell" lint). `value_type_kind_matches` accepting Undef for any type (`reify-eval/src/lib.rs:222`) stays — it IS the degradation contract; only the *reporting* gets honest here (task ι).
- **Tensor static M×N shape** — `Type::Tensor` carries a single `n`; row count discarded (math_signatures D5). Stub PRD: `docs/prds/v0_6/tensor-static-shape.md` + bookmark task.
- **Ambient default Material** mechanism — companion PRD `docs/prds/v0_6/ambient-default-material.md` (owns the water→error flip).
- **Geometry-handle arg resolution** (`resolve_geometry_handle_arg` `:4208-4217`, `resolve_parent_geometry_handle_arg` `:4228-4252`) — owned by unified-build-DAG ε=4358 (cross-sub/IndexAccess shapes + `E_EVAL_*` diagnostics).
- Structural tensor equality (decision 3 — future non-breaking widening).
- Constraint-aware / L2 type-param resolution (4431-4438 batch).

## 5. Pre-conditions

- α4373 (`Type::Real` deletion) for tasks α/β only — wired as real dep edges.
- Everything else: substrate verified present 2026-06-10 (check sites, ValueMap access at dispatch, `implicitly_converts_to`, Kleene module). **No novel grammar — G3 N/A** (polymorphic zero is new *semantics* for existing literal syntax; `--strict` is a CLI flag; conformance rule is checker-internal).

## 6. Cross-PRD relationships (G4 seam table)

| Seam | Direction | Mechanism | Owner |
|---|---|---|---|
| unified-build-DAG (4357-4362) | complement | geometry-handle resolvers + `FunctionCall`-args recursion in `rewrite_geometry_queries` | **ε=4358** owns those two resolvers + their diagnostics; THIS PRD owns the other 12 resolvers + the acceptance contract. Survey 2026-06-10: zero file-region collision; if dispatched concurrently, orchestrator file locks on `geometry_ops.rs` serialize them. |
| real-dimensionless (4372-4377) | consumes | canonical dimensionless type after `Type::Real` deletion | 4373 owns the deletion; THIS PRD's α/β depend on it. β does NOT touch value-layer canonicalization (4374's). |
| structural-traits-reconciliation δ=4229 | produces | density contract (γ supersedes 4473; 4229's dep edge repointed 4473→γ at decompose) | γ. Interaction: Cluster 1 makes 4229's OLD scalar `> 0` constraint a *loud* error against a tensor member — consistent; 4229 replaces it in the same change. Post-β, 4229's PD constraint may use bare `> 0`. |
| ambient-default-material PRD | produces interim / consumes flip | `resolve_body_density` water rung | THIS PRD keeps warn+water (δ); ambient PRD owns rung swap + hard-error flip. |
| tensor-static-shape stub PRD | produces pointer | static shape rejection for comparisons/matrix() | stub PRD (bookmark task). |
| auto-type-param batch (4431-4438) | none (gradualism boundary) | guards skip `TypeParam`/unresolved types (decision 6) | each owns its side; no file-region overlap in the binop guard region. |
| 4226 / esc-3115-112 | reverses convention | dimensioned-zero RHS boilerplate | β's migration sweep deletes it (deliberate re-litigation, Leo 2026-06-10). |

## 7. Contract section (H)

### 7.1 Compile-time operand guard contract (`expr.rs` binop region)

For `BinOp::{Eq,Ne,Lt,Le,Gt,Ge}` with compiled operand types `L`, `R` (after β's zero-coercion rewrite):
- If either ∈ {`Type::Error`, `TypeParam`/unresolved} → **no diagnostic** (poison/gradualism).
- Else if either is non-scalar-kind (Tensor/Matrix/Vector/Point/List/Enum/String/…) → **error** `E_CmpOperandKind`: names the op, the offending operand's type, and (for tensor/matrix) fixits `eigenvalues(x)[0]`, `trace(x)`.
- Else if both scalar-kind and dimensions differ → **error** `E_CmpDimensionMismatch` (reuses Add/Sub formatting, `format_dimension_mismatch_diagnostic`).
For `BinOp::{And,Or}`: non-Bool, non-Error/TypeParam operand → **error** (generalizes `ImpliesRequiresLogical`; update the `:1158-1159` comment). **Invariant: no runtime behavior change for code that compiles** — Kleene eval untouched.

### 7.2 Polymorphic-zero contract (compile-time literal rewrite)

In a BinOp of comparison or Add/Sub kind, if exactly one operand is a syntactic literal `0`/`0.0` (optionally unary-negated) typed dimensionless and the other types `Scalar<D>` (D ≠ dimensionless): rewrite the literal to `Scalar<D>(0.0)` (type + value). Applies before the §7.1 guard. Never fires for non-zero literals, non-literal zeros, or non-scalar siblings. Runtime layers see an ordinary dimensioned scalar.

### 7.3 Value-level acceptance contract (eval layer)

One helper family, used by every recognized-builtin arg site this PRD owns:
```
accept_arg(value: &Value, spec: ArgSpec) -> Result<Accepted, ArgRejection>
// ArgSpec = expected kind + DimensionVector (+ shape where applicable)
```
- Arg **expressions are evaluated to Values** first (`eval_expr` against the ValueMap — the `eval_named_arg` pattern). `CompiledExprKind` shape-matching is dead in owned resolvers.
- `Value::Undef` input → return Undef quietly (data-indeterminacy degradation, unchanged).
- Defined value, wrong kind/dimension → `ArgRejection` which the dispatch site MUST surface as a diagnostic: `<builtin>: argument <n> (<name>) expects <expected>, got <actual>` (+ migration hint where applicable). **Silent fall-through on a recognized builtin is a contract violation.**
- Density instance: `ArgSpec{Scalar, MASS_DENSITY}`; bare-Real rejection carries the `7850kg/m^3` migration hint; shared by Contract A (γ) and Contract B (δ).

### 7.4 Conformance collision contract (checker phase 5)

After the required-members loop: for every trait default (param or let) whose name collides with a conformer member, `implicitly_converts_to(conformer_type, trait_declared_type)` must hold, else error `type mismatch for trait member '<name>': expected <T>, got <U>` (same wording as the required-member arm). Compatible collision → existing inject-skip behavior unchanged.

## 8. Reference inventory

### 8.1 Owned shape-matching resolvers (replaced under §7.3)
`resolve_bare_angle` :418-439 · `resolve_int_value_ref` :2487-2512 · `resolve_point3_length_arg` :3894-3941 · `resolve_density_arg` :3961-3987 · `resolve_real_scalar_arg` :3996-4012 · `resolve_vec3_arg` :4038-4057 · `resolve_angle_scalar_arg` :4066-4071 · `resolve_length_scalar_arg` :4080-4085 · `resolve_scalar_bound_expr` :4105-4117 · `resolve_range_dim_arg` :4134-4166 · `resolve_owner_solid_handle` :4186-4202 · `resolve_string_literal_arg` :4893-4898 (all `crates/reify-eval/src/geometry_ops.rs`; ~35 builtins). Excluded (4358's): `resolve_geometry_handle_arg` :4208-4217, `resolve_parent_geometry_handle_arg` :4228-4252.

### 8.2 Bare-Real density migration surface (γ)
`examples/kernel_queries/moment_of_inertia_box.ri` (incl. stale grammar note :16-19) · `examples/kernel_queries/all_queries_walk.ri` (:127-132) · `examples/topology_selectors/block_inertia.ri:30` · `examples/topology_selectors/all_topology_selectors_wiring.ri:50-51` · `crates/reify-eval/tests/kernel_queries_moment_of_inertia_smoke.rs` · `crates/reify-eval/tests/topology_selector_smoke_tests.rs` · `crates/reify-eval/tests/topology_selectors_tests.rs` · `crates/reify-eval/tests/topology_selector_runtime.rs:483-495` · `geometry_ops.rs` unit tests :5938-6076 · doc-comments `units.rs:158-159`, `geometry_ops.rs:3649`, `:3943-3960`. (Compound-unit literals `7850kg/m^3` parse today — the "v0.3 grammar restriction" those fixtures document is OBE.)

## 9. Decomposition plan

Greek letters; real IDs at decompose. **Spine:** β → α; γ → {δ, ε, ζ}; all → λ.

- **β — polymorphic literal zero + boilerplate sweep.** Modules: reify-compiler (expr.rs), stdlib `.ri`, examples. Deps: **4373** (out-of-batch). Signal: `constraint mass > 0` compiles and reports OK/VIOLATED (today: compiles, permanently INDETERMINATE); stdlib diff deletes the `0.0 * 1kg * 1m * 1m` RHS boilerplate.
- **α — relational + logical operand guards.** Modules: reify-compiler (expr.rs, type_compat.rs), tests. Deps: β (so bare-zero comparisons never transit an error state), transitively 4373. Signal: `constraint t > 0 * 1kg*1m*1m` on a tensor → compile error with eigenvalues/trace fixit (today: zero diagnostics, permanent INDETERMINATE); `5 and x` → compile error; dimension-mismatched scalar compare → compile error; existing Kleene runtime tests untouched.
- **γ — ONE density contract, part A (supersedes 4473).** Shared `accept_arg` density spec; Contract A flips to Density-only; bare Real → migration diagnostic; silent fall-through on the two helpers killed; full §8.2 migration. Modules: reify-eval (geometry_ops.rs), reify-compiler (units.rs docs), examples, tests. Deps: none. **Consumer: 4229 (dep edge repointed here).** Signal: `let d = material.density; let i = moment_of_inertia(b, d)` evals to the non-Undef analytic tensor (probe-5-verified values); bare-Real density → diagnostic naming `7850kg/m^3`.
- **δ — density contract, part B (body_mass_props hygiene).** `cell_f64`-verbatim hole → dimension-checked via shared helper (Pressure-as-density → loud error); explicit-arg-ignored hole → loud diagnostic; water rung KEPT (warn) per decision 9. Modules: reify-eval (dynamics_ops.rs), tests. Deps: γ. Signal: `body_mass_props(b, youngs_modulus_value)` → diagnostic "expects Density, got Pressure" (today: silently treated as kg/m³); ladder + `W_DynamicsDefaultDensity` behavior otherwise byte-identical.
- **ε — uniform evaluate-then-accept for owned resolvers.** §8.1 resolvers route through arg-expression evaluation + `accept_arg`; silent fall-through dead on all owned recognized builtins. Modules: reify-eval (geometry_ops.rs). Deps: γ (helper module). Signal: `moment_of_inertia(b, 7850kg/m^3)` **inline literal** works end-to-end (today: silent Undef — the documented `:3970-3974` contract); any unsupported defined arg value on an owned builtin → diagnostic naming builtin/arg/expected/got.
- **ζ — compile-time per-arg signatures for builtin families.** Arg-slot table beside `math_signatures.rs` result types; call-site check (definite mismatches only, decision 6); `density: Density` enforced statically. Modules: reify-compiler (units.rs / math_signatures.rs / expr.rs call-site). Deps: γ (contract definition). Signal: `moment_of_inertia(b, 7850.0)` → **compile-time** diagnostic naming Density before any eval; wrong-dimension tolerance arg to `faces_by_normal` → compile diagnostic.
- **η — conformance collision rule.** §7.4 in checker phase 5. Modules: reify-compiler (conformance/checker.rs), tests. Deps: none. Signal: the probe-7 fixture (scalar override vs tensor-defaulted trait param) → loud type-mismatch error (today: silently accepted); a type-compatible override still conforms cleanly (both directions pinned).
- **θ — `reify check --strict`.** Non-zero exit when any constraint INDETERMINATE; lists which + why. Modules: reify-cli (main.rs:539,656,831 region). Deps: none. Signal: `reify check --strict` on a model with an indeterminate constraint exits non-zero naming it; without `--strict`, behavior unchanged.
- **ι — honest indeterminacy diagnostics (Undef-taxonomy slice).** Constraint reporting distinguishes "operator undefined for these operand kinds" from "undefined inputs: <cells>" (checker has expr + values; inspect leaf definedness). Modules: reify-constraints (lib.rs:75-86), reify-expr if needed. Deps: none. Signal: a constraint over genuinely-Undef inputs names the undefined cells; one whose inputs are all defined but operator-undefined says so (today: both claim "undefined inputs").
- **κ — MassProperties.inertia dimension tightening.** `Matrix<3,3,Real>` → `Matrix<3,3,MomentOfInertia>` (`dynamics.ri:71-83`); populate site (`dynamics_ops.rs:284-392`) + RNEA extraction (`reify-stdlib/src/dynamics/eval.rs:314-330`) updated. Modules: stdlib dynamics.ri, reify-stdlib, reify-eval. Deps: none (anchors post-4229 consistency; no hard edge). Signal: RNEA `inverse_dynamics` examples produce **identical** numeric output (pure type-layer change — si_values untouched); member statically dimensioned.
- **λ — integration gate (CRITICAL leaf).** The §10 boundary-test table as committed tests + one CI example `.ri` exercising: bare-zero constraint OK/VIOLATED, tensor-compare compile error + fixit, inline dimensioned density end-to-end, Pressure-as-density rejection, conformer-collision error, `--strict` exit code. Deps: α β γ δ ε ζ η θ ι κ. Signal: the example runs in CI; every §10 row green.

## 10. Boundary-test sketch (two-way)

| # | Scenario | Pre | Post |
|---|---|---|---|
| 1 | tensor `>` scalar constraint | defined `Tensor<2,3,MOI>` member | compile error E_CmpOperandKind + eigenvalues/trace fixit; NOT indeterminate-at-runtime |
| 2 | `mass > 0` bare zero | `mass : Scalar<Mass>` | compiles; OK for positive, VIOLATED for negative (probe both) |
| 3 | `0` vs tensor | tensor member | NO zero-adoption; α's kind error fires |
| 4 | `x and 5` | x : Bool | compile error; Kleene tests (`false ∧ Undef = false`, `true ∨ Undef = true`) still pass verbatim |
| 5 | TypeParam-typed operand in compare | generic/auto context | NO diagnostic (gradualism, decision 6) |
| 6 | `moment_of_inertia(b, material.density)` via let | Material with Density | non-Undef analytic tensor (probe-5 values) |
| 7 | `moment_of_inertia(b, 7850.0)` | — | ζ: compile diagnostic; ε: runtime diagnostic w/ migration hint; never silent |
| 8 | `body_mass_props(b, <Pressure>)` | dimensioned non-Density | loud "expects Density, got Pressure"; today silently 200e9 kg/m³ |
| 9 | `body_mass_props(b)` no Material | body without material | warn `W_DynamicsDefaultDensity` + water — UNCHANGED (interim, decision 9) |
| 10 | scalar override vs tensor-defaulted trait param | probe-7 fixture | η: loud type mismatch; type-COMPATIBLE override still conforms |
| 11 | conformer param shadows trait let, compatible type | measured-value override | accepted (override idiom preserved) |
| 12 | `reify check --strict` w/ unrealized geometry | indeterminate constraint | exit ≠ 0 naming the constraint; without flag: exit 0, summary line unchanged |
| 13 | defined-inputs operator-undef vs undef-inputs | ι fixtures | distinct messages |
| 14 | RNEA after κ | existing dynamics example | numerically identical output |

## 11. Open questions (tactical)

1. **Diagnostic code names** (`E_CmpOperandKind` vs folding into `DimensionMismatch`; W vs E for ε's runtime rejections). Suggested: new E codes for compile guards, W for runtime arg rejections (degradation still Undef). Decide in α/ε.
2. **`--strict` spelling** (flag vs config vs env). Suggested: flag only, v1. Decide in θ.
3. **ζ table placement** (extend `math_signatures.rs` vs new `builtin_signatures.rs`). Decide in ζ.
4. **β sweep breadth** — migrate examples too or stdlib only? Suggested: both (examples are docs). Decide in β.
