# Builtin-signature registry: one enum-keyed source of truth for builtin names, signatures, result types, and dispatch (compiler-type-hygiene Wave 3)

**Milestone:** v0_6 · **Status:** active · **Date:** 2026-08-04 · **Approach: B+H** (blast radius: new `reify-builtins` + reify-core/compiler/stdlib/expr/eval/lsp; the builtin-signature seam is load-bearing) · Resolves bookmark **#5068** (compiler-type-hygiene δ). INV-COMP-2 end-state owner: registration drift becomes **unrepresentable by construction**, replacing disjointness-by-test.

Evidence base: the 2026-08-03 measured enumeration (fallback-soundness investigation, `docs/notes/fallback-soundness-analysis-2026-08-03.md`; the companion `docs/notes/fallback-soundness-xref-2026-08-03.json` holds only the two raw name sets the set-diff was taken over — `eval` 231, `fallback` 121 — the per-name classification is prose in the analysis). Headline, all [verified] against main `36738b9b92`: 231 eval-dispatchable builtins; 110 typed explicitly by the `NoUserFunctions` ladder (`expr.rs:3066-3604`); 121 ride the terminal first-arg fallback (`expr.rs:3568-3584`); **93 mistyped** — including wrong *values*, not just types (`floor(2.5mm)` → `Int(0)`, floor of the SI-erased f64); 16 correctly first-arg-typed; 5 dimension-dependent; 7 shadowed by typed stdlib `.ri` decls. The LSP `BUILTIN_FUNCTIONS` table advertises 35 signatures the compiler contradicts. A further 127 compiler-registered names have no `eval_builtin` arm (geometry ops, selectors, relations — engine-dispatched or compile-only). Cross-crate agreement today is one drift test (task 5055, `crates/reify-eval/src/registry_drift_tests.rs`) whose declared blind spot is exactly the zero-registry class, plus 32 hand-written disjointness tests and prose "maintenance contract" comments.

## 1. Goal

A `.ri` author gets **correct static types for every builtin** — and the workspace makes signature drift a compile error, not a latent user-facing lie:

- `let t = frame_to_frame(a, b)` types `Transform3`; passing `t` to `fn f(t: Transform3)` resolves (today: typed `Frame(3)`, false "no matching overload").
- `floor(2.5mm)` is a compile diagnostic with a fixit hint (today: silently types `Scalar<LENGTH>` and evaluates `Int(0)`); `floor(2.7mm, 0.5mm)` evaluates `2.5mm` (new two-arg form).
- `orient_to_axis_angle(q).angle` types `Angle` via a nominal structure (today: untyped Map lookup).
- LSP hover/completion signatures are **rendered from the registry** and cannot disagree with the type checker (today: 35 contradictions).
- Adding a builtin name to eval without full registration **does not compile**; registering a row without an eval binding **does not compile**.

## 2. Background

The terminal first-arg fallback is an M1 fossil (commit `a32a1b2b2e`, "for math functions, use the type of the first argument as a heuristic") whose original clientele was carved out into `math_signatures.rs` by tasks 4179/4182/4352. Since then every new family PRD has hand-carved names out of it; the enumeration shows what remains is 77% wrong. The lie persists into `cell_type`, misroutes the ε1 MemberAccess dispatch, causes false overload failures, gets GUI param-overrides rejected at runtime (`value_type_kind_matches` runs only on override paths — `joint_signatures.rs:15-28`), and forced the deliberate weakening of the `let`-annotation checker (`entity.rs:556`). Interim closure is Leo-ratified and filed: task 5371 (closed-world manifest + type-preserving allowlist + warn-mode `UnresolvedFunction` diagnostic) and task 5997 (Error flip, gated on RU #5517/#5518 + #5380). This PRD is the durable end-state those tasks' texts already cite.

## 3. Resolved design decisions (Leo, 2026-08-03/04)

1. **Enum-keyed, routed dispatch.** A `registry!` declaration in a new leaf crate **`reify-builtins`** generates `enum BuiltinId` (one variant per row), the row table, and the **single** `lookup(name, argc) -> Option<BuiltinId>` — the only string→builtin resolution in the workspace. Eval dispatch becomes an exhaustive `match BuiltinId` with **no `_` arm** in each binding crate; the compiler's result-type computation becomes a **total function over `BuiltinId`**. Membership and dispatch drift are unrepresentable: a row without an eval arm fails the owning crate's build; an eval arm without a row has no variant to match.
2. **Compile-time binding.** The compiler resolves `name → BuiltinId` once (at the current ladder position, after user/stdlib-fn resolution) and stores the id in the `CompiledExpr::FunctionCall` node; eval dispatches on the id. String matching leaves the eval path entirely by end of migration.
3. **Crate placement.** `reify-builtins` depends **only on `reify-core`** (for `Type`); it holds rows, `BuiltinId`, arity/arg-slot specs, and result-type resolvers. It holds **no `Value` and no fn pointers** — eval binding is the exhaustive match in the owning crate (`reify-stdlib`, `reify-expr`, `reify-eval`). Consumers: reify-compiler, reify-stdlib, reify-expr, reify-eval, reify-lsp.
4. **Binding-kind column** covers the full ~358-name surface in one table: `EvalBuiltin` (stdlib `eval_builtin` arms), `ExprIntercept` (reify-expr native), `EnginePostProcess` (geometry queries/selectors/kinematics via reify-eval maps), `GeometryOp` (lowered to `CompiledGeometryOp`), `CompileOnly` (relations, markers). Exhaustiveness is enforced **per kind** in the kind's owning crate.
5. **Result-type column**: `Const(Type)` | `ArgAware(fn(&[Type]) -> Option<Type>)`. An `ArgAware` returning `None` (mis-shaped args) emits a **diagnostic** (`E_BuiltinArgShape` family) — never a silent first-arg guess. The existing per-slot dimension checks (`check_builtin_arg_types` vocabulary) migrate into the arg-slot column.
6. **Dimensioned-numeric rulings.** Runtime scalars are SI-normalized (spelled units do not survive to eval — verified by the `floor(2.5mm)`→`floor(0.0025)` probe), so unit-respelling invariance is inherent and a bare dimensioned `floor` has no coherent answer. Therefore: `floor/ceil/round(x: Scalar<DIMENSIONLESS>) -> Int`, dimensioned arg → diagnostic with hint; **new two-arg form** `floor(x: Scalar<D>, quantum: Scalar<D>) -> Scalar<D>` = `floor(x/quantum)·quantum`, any positive same-dimension quantum (`floor(2.7mm, 0.5mm)` == `2.5mm`; `quantum <= 0` → eval diagnostic). `sinh/cosh/tanh(x: Scalar<DIMENSIONLESS>) -> Real`. `log10` dimensionless-only — log-of-dimension is unrepresentable in the rational-exponent dimension algebra and low-utility; the hint teaches `log10(x/1mm)`.
7. **Heterogeneous `Value::Map` returns → nominal stdlib structures, per family** (the joint precedent: eval returns a Map, the compiler types a `StructureRef`): `AxisAngle { angle: Angle, axis: Vector3 }` for `orient_to_axis_angle`, `Twist { angular, linear }` for `transform_log`/`transform_exp`, flexure/stackup/world result structures likewise. This **resolves task 5380's open rulings** to a single pattern.
8. **LSP generated from the registry.** Signature text is rendered mechanically from rows; curation (which builtins surface) and doc prose stay LSP-side keyed by `BuiltinId`. Subsumes the signature halves of tasks 5704/5707/5922.
9. **Registry never claims `.ri`-owned names.** `compose` is removed from `FIELD_OP_NAMES` (it is a typed generic stdlib `.ri` fn, task 4224); invariant: registry names ∩ typed-stdlib-`.ri`-decl names = ∅ (the 7 SHADOWED names stay `.ri`-owned).
10. **Explicit name rows, never prefix rules** (`orient_*`/`frame_*`/`transform_*` are not type-uniform — norm 9bef6759).
11. **Migration is family-by-family**, each swap validated by the 5055 drift test green before AND after, and each swap **deletes** the units.rs disjointness tests it subsumes (32 tests + `signatures_common.rs` fully retired at the end). Precedent: the geometry-op dispatch-registry program (tasks 4670-4675).
12. **Executed static-vs-runtime parity harness driven by the registry** (no second derivation): for every `EvalBuiltin` row, synthesized args per arg-slots → eval → returned `Value` kind must satisfy `value_type_kind_matches` against the declared result type; divergences allowed only via one commented exemption ledger. This closes the residue the table alone cannot: a buggy eval body returning the wrong kind.
13. **Fail-closed rollout unchanged.** 5371 (warn) → 5997 (error) proceed as filed; the registry migrates independently. The terminal first-arg fallback (and 5371's interim allowlist family — its 16 names get real rows in their families' migrations) is **deleted as the final step**, gated on all families migrated + 5997.

## 4. Out of scope (named)

- **Geometry value-position result typing**: the 66 `GEOMETRY_FUNCTION_NAMES` keep the documented `dimensionless_scalar()` placeholder in value position (rows carry binding-kind `GeometryOp` with the placeholder ledgered, incl. the `sweep` mechanism/geometry name collision). Real fix needs geometry-type design — a future PRD.
- **Un-annotated user-fn return-type reconciliation** — task #5991, different seam.
- **Overload-resolution algorithm unification** — #5992's breadcrumb; the registry carries builtin signatures, not user-fn overload semantics.
- **New eval capabilities**: `piecewise_polynomial` stays a stub (row ledgered with a PTODO cite); no new geometry/FEA behavior.
- **LSP doc-prose authoring** beyond mechanical signature derivation.

## 5. Pre-conditions

All verified 2026-08-04 unless noted: no novel grammar (the two-arg `floor` is ordinary call syntax — G3 N/A for syntax); `registry!` is `macro_rules`-representable (no new proc-macro crate expected; strum precedent for derives, workspace dep present); reify-eval already deps reify-compiler (drift test); reify-compiler dev-deps reify-expr/reify-eval (parity harness placement options open). Task **5344** (orientation/frame ctor registrations, in-progress) must land before the orientation family migration — its arms are absorbed as seed rows. Task **5371** supplies the compile-side manifest; migration converts manifest entries to rows (coordinate, not hard-block: the enumeration `xref.json` is equivalent seed data).

## 6. Cross-PRD relationships (G4)

| Other PRD / task | Direction | Seam mechanism | Owner |
|---|---|---|---|
| compiler-type-hygiene.md wave 2 (parent) | resolves its δ slot #5068 | 5055 drift test = per-family migration validator, retired/reduced at end | this PRD |
| 5371 / 5997 (interim closure) | consumes manifest; supersedes at end | `EVAL_DEFERRED_BUILTIN_NAMES` → rows; final fallback deletion removes the interim allowlist; 5997's `E_UnresolvedFunction` stays for lookup-miss names | 5997 owns severity flip; this PRD owns fallback deletion (task ω) |
| stdlib-namespace (5493-5505) | produces for κ #5503 | `reify_builtins::lookup` is the builtin-membership authority κ's strict-visibility flip needs | this PRD produces; κ consumes |
| resolution-unification (5515-5529) | upstream only | compile_program shrinks ladder input; gates 5997, not this PRD | RU |
| 5704 / 5707 / 5922 (LSP parity) | subsumed (signature halves) | decision 8 generation; port-before-cancel at decompose | this PRD |
| 5380 (fallthrough inventory rulings) | resolved by decision 7 | nominal structures; folded into orientation/numeric family tasks | this PRD |
| geometry-op dispatch registry (4670-4675, done) | bridged | `GeometryOp` rows name-set-checked against the reify-ir descriptor table (one bridging test — no third registry) | this PRD |

No new contested-ownership pair (checked against the overlay's three known pairs).

## 7. Contract (H)

### 7.1 Row shape

`{ name: &'static str, id: BuiltinId (generated), family: Family, binding: BindingKind, arity: Arity (Exact|Range|Variadic), arg_slots: &[ArgSlot], result: ResultSpec }` with `ResultSpec = Const(Type) | ArgAware(fn(&[Type]) -> Option<Type>)`. Same-name arity overloads (`offset`@2/@3, `floor`@1/@2) are distinct rows in one name-group; `lookup(name, argc)` disambiguates. Same-name same-arity kind overloads (`union`/`difference`/`intersect` CSG-vs-selector) are one row whose `ArgAware` resolver dispatches on operand kinds — the row's doc names both dispositions.

### 7.2 Invariants

- **I-REG-1**: `reify_builtins::lookup` is the only string→builtin resolution in the workspace; no `"name" =>` string dispatch on builtin names outside `reify-builtins` (grep-gate; `.ri`-decl shadow set excluded).
- **I-REG-2**: every `BuiltinId` is bound exactly once in its `BindingKind`'s owning dispatcher, via exhaustive match with no `_` arm.
- **I-REG-3**: compiler result-typing is total over `BuiltinId`; the only non-registry outcomes of compiling a call are: user/stdlib-fn resolution, `E_BuiltinArgShape`/slot diagnostics + `Type::Error` poison, or (pre-ω) the 5371/5997 unresolved-name path.
- **I-REG-4**: the parity harness derives entirely from rows (synthesized args per arg-slots → eval → `value_type_kind_matches` vs declared result); one commented exemption ledger; a mutation recipe (flip one row's result) is documented and fails.
- **I-REG-5**: registry names ∩ typed-stdlib-`.ri`-decl names = ∅ (locking test).
- **I-REG-6**: LSP signature text is derived from rows, never authored (the curated table holds only `BuiltinId` + prose docs).

### 7.3 Migration-step contract (per family τ)

(1) rows added; (2) compiler family arm replaced by registry lookup; (3) eval dispatcher arm replaced by `BuiltinId` match; (4) covered disjointness tests deleted + drift-test ledger updated; (5) parity rows active; (6) examples-corpus diagnostics byte-identical **except** corrections enumerated in the task text (each formerly-mistyped name's new type is a listed, reviewed change). New gate-resident tests carry their drift-guard registrations same-diff (run-all-classification manifest, wallclock bounds, nextest partitions).

## 8. Boundary-test sketch (two-way)

| # | Scenario | Pre (verified today) | Post |
|---|---|---|---|
| 1 | `frame_to_frame(a,b)` passed to `fn f(t: Transform3)` | typed Frame(3); false NoMatch error | resolves; hover shows `-> Transform3` |
| 2 | `floor(2.5mm)` | silent `Scalar<LENGTH>`; evals `Int(0)` | compile diagnostic + two-arg hint |
| 3 | `floor(2.7mm, 0.5mm)` | (form does not exist) | evals `2.5mm`, typed `Scalar<LENGTH>` |
| 4 | `sinh(1mm)` / `log10(2mm)` | silent; evals erased-SI Real | compile diagnostic (dimensionless-only) |
| 5 | `orient_to_axis_angle(q).angle` | Map; untyped member | `Angle` via `AxisAngle` structure |
| 6 | registry row added, no eval arm | (n/a) | owning crate build FAILS (doc-recipe, α) |
| 7 | eval arm added, no row | silent fallback typing | unrepresentable (no `BuiltinId` variant) |
| 8 | `line(p1, p2)` (nonexistent) | compiles silently | `E_UnresolvedFunction` (5371 warn → 5997 error; lookup-miss path preserved post-ω) |
| 9 | parity: row result mutated | (n/a) | harness RED (mutation recipe) |
| 10 | examples corpus (`point3` in 35 files) | current diagnostics | byte-identical except enumerated corrections per τ |
| 11 | `compose(f,g)` with std.fields in scope | resolves to `.ri` fn | unchanged; out of scope → unresolved-name path (family membership dropped) |
| 12 | LSP completion signature for any registered name | 35 contradict compiler | string-equal to registry rendering (derived) |

## 9. Decomposition plan

Spine: α → β → τ* (pipeline, independent where files disjoint) → π/ψ grow per τ → ω → λ. Docs-truth leaves ρ ride the τ that changes each surface. Real IDs at decompose; `metadata.files` tight-or-empty.

- **α — `reify-builtins` crate + `registry!` macro + `BuiltinId` + `lookup` + seed families (parse: 2 names, analysis: 5).** End-to-end for the seeds: compiler arms swapped, stdlib match swapped, negative recipe (row-without-arm build failure) documented. Signal: `parse_length("3mm")` types `Option<Length>` via the registry (CLI check); boundary #6.
- **β — compiler consumes the registry ahead of unmigrated family arms + `CompiledExpr` carries `BuiltinId`.** Coexistence: registry lookup first, legacy arms until their τ. Investigate content-hash implications of the id field (result_type is currently unhashed). Signal: corpus byte-identical; seed families dispatch by id (assert in a debug test).
- **τ-numeric** — floor/ceil/round dual-form (new two-arg eval impl) + sinh/cosh/tanh + log10 + mod/remap rows, diagnostics, corpus corrections; cites and closes the `math_signatures.rs:90-92` prose deferral. Signals: boundaries #2-#4.
- **τ-orientation/frames** — after #5344 lands; absorbs its arms as rows; `AxisAngle`/`Twist` structures (folds #5380). Signals: #1, #5.
- **τ-joints**, **τ-fea/flexures/stackup** (result structures per decision 7), **τ-mechanism/trajectory** (stub ledger for `piecewise_polynomial`), **τ-complex/re-im**, **τ-datums/affine/list/field** (drops `compose` per decision 9; boundary #11), **τ-queries/selectors** (`EnginePostProcess` kind; reify-eval maps swap), **τ-relations/markers** (`CompileOnly`), **τ-geometry-ops** (`GeometryOp` kind + bridging test to the reify-ir descriptor table).
- **π — LSP generation** (decision 8; subsumes 5704/5707/5922 signature halves — port-before-cancel at decompose). Signal: boundary #12.
- **ψ — parity harness** (I-REG-4), lands with α's seeds, grows per τ. Signal: boundary #9 mutation recipe.
- **ω — delete the terminal first-arg fallback + the 5371 interim allowlist family.** Deps: all τ + #5997. Signals: I-REG-1/-3 grep-gate + locking test; boundary #8 end-state.
- **λ — integration gate**: §8 rows green in CI on one commit; 32 disjointness tests + `signatures_common.rs` confirmed deleted; drift test retired to registry-invariant form.
- **ρ — docs-truth leaves** (per overlay gate, same-diff or hard-dep with their τ): doc-chunk updates for corrected signatures + the two-arg floor form (registry-verified against rows), `examples/best_practices/` exemplar + INDEX line for the quantized-floor idiom, reify-design cheatsheet index line, intent-level discoverability acceptance.

## 10. Open questions (tactical)

1. **`registry!` implementation form** — `macro_rules` vs `build.rs` codegen. Suggested: `macro_rules`. Decide in α.
2. **`lookup(name, argc)` vs `lookup(name) -> NameGroup`** — decide in α with the overload rows in hand.
3. **`BuiltinId` in `CompiledExpr`: inline field vs side table**, and whether it joins the content hash (result_type currently unhashed — cache-identity implications). Decide in β.
4. **Structure names/fields for decision-7 nominal types** (`AxisAngle` vs `OrientAxisAngle`, stackup/flexure result shapes) — per-τ, mirroring stdlib naming conventions.
5. **Parity-harness crate placement** (reify-eval in-crate module beside the drift test vs reify-compiler tests dir) — decide in ψ.
6. **`Int` vs `Real` for 1-arg floor family return** — current eval returns `Int`; keep unless τ-numeric's corpus sweep surfaces a consumer needing `Real`. Decide in τ-numeric.
