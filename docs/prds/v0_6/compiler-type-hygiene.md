# Compiler type hygiene, wave 2: trait type-arg rejection, Mul/Div truth-table alignment, registry drift guard, exhaustive MemberAccess dispatch, auto_free dedup

**Milestone:** v0_6 · **Status:** active · **Date:** 2026-07-06 · **Approach: B+H** (blast radius: reify-compiler / reify-core / reify-expr / reify-eval; operator-typing seam is load-bearing)

Successor wave to `docs/prds/v0_6/type-hygiene.md` (tasks 4490/4493 series, all done) in the **hotspot program** (survey `docs/notes/bug-hotspot-survey-2026-07-05.md` §H5 + latent-bugs 2/3; spawn brief `~/.claude/spawn-briefs/prd-compiler-type-hygiene.md`). Invariants established: **INV-COMP-1, INV-COMP-2 (interim), INV-COMP-3** (`docs/invariants.md`) — every task cites its INV-id(s) with the enforcement mechanism in done-criteria (INV-META-1). All file:line anchors re-verified 2026-07-06 against main `4d696e63cb`; both latent bugs re-confirmed by executable probe (fixtures `tests/prd-gate/fixtures/compiler_type_hygiene_*.ri`, transcripts in §2).

## 1. Goal

A Reify author can no longer hit the two H5 silent-drop bugs, and the structural causes behind expr.rs/entity.rs's 43–47% fix ratios get their Wave-1/2 hardening:

- `param m : SomeGenericTrait<Foo>` is an **explicit compile-time rejection** naming the trait and its args (today: `reify check` exits 0, `All constraints satisfied.`, `Foo` silently dropped — bare `Type::TraitObject`, no arity check).
- Runtime-legal vector/tensor/point/complex/transform arithmetic **compiles with the correct static result type**; runtime-unsupported operand combinations are **compile errors** (today: `a * a` on `Vector3<Length>` silently types `Int` and evals Undef; worse, `a * 2.0` types `Scalar` — so `constraint s > 0` silently defeats the 4490 comparison guard and is permanently indeterminate).
- Adding a builtin name to a compiler-side name family without its eval-side siblings (or vice versa) **fails CI immediately** (today: one selector is registered in ≥7 places across 2 crates; a missed registration degrades to silent Undef; the only guard is a prose "Maintenance contract" comment).
- The MemberAccess receiver dispatch is **one exhaustive match** over `Type` — a new `Type` variant forces a compile-time decision (today: ~1300 lines of sequential if-guards, 39-variant `Type`, nothing trips).
- The `auto_free` decl-construction block exists **once** (today: three structural copies).

## 2. Background — evidence

**Latent bug (a): trait type-args silently dropped.** `resolve_type_expr_with_aliases_kinded` intercepts structure-with-args at `type_resolution.rs:1704-1724` (the 4603 γ un-drop fix → `Type::Applied`), but a trait name with non-empty `type_args` falls through to the simple-name resolution at `:1728`, which returns bare `Type::TraitObject(name)` from the trait-name fallback at `:658-662` — args dropped, no diagnostic, no arity check. `check_applied_type_arg_bounds` (`:3801`, E_TYPE_ARG_ARITY/E_TYPE_ARG_BOUND) only sees `Type::Applied` nodes, i.e. structures. Probe (2026-07-06): `reify check` on `param m : SpecLike<Foo>` → exit 0, zero diagnostics. Note the enum precedent at `:1738-1752`: `Enum<Args>` deliberately keeps `type_args` non-empty and falls through to an entity.rs "enum does not accept type arguments" error — traits are the only namespace that resolves-and-drops.

**Latent bug (b): Mul/Div static table diverges from the runtime truth table.** `infer_binop_type` (`type_compat.rs:1538`) Mul arm `:1576-1589`, Div arm `:1590-1596`: only (Scalar,Scalar) is dimension-correct; `(Scalar, other)` "preserves the scalar type" (wrong for every scale operation — runtime returns the *aggregate*); everything else falls to `_ => Type::Int`. The runtime (reify-expr `eval_mul` `:4354-4629`, `eval_div` `:4631-4780`) supports a much richer intentional table — Complex×{Complex,Scalar,Int,Real}, Tensor/Vector/Point scaling (commutative for Mul, left-only for Div), Transform×Vector/Point/Transform, reciprocal-dimension Real/Scalar division — and returns `Undef` for the rest (Vector×Vector, Tensor×Tensor, Matrix×anything, …). Guards exist for Add/Sub (`expr.rs:1728`), Mod (`:1705`), Pow (`:1611`), comparisons (`:1781`, 4490 α), logical (`:1806`) — Mul/Div have none. Probes (2026-07-06): `let v = a * a` on `Vector3<Length>` → `reify check` exit 0 silent, eval `P.v = undef` (OpContractViolation); `let s = a * 2.0; constraint s > 0` → compiles silently (s statically `Scalar`), eval `P.s = vec(2 m, 4 m, 6 m)` and the constraint is *permanently indeterminate* — the Mul mistyping re-opens the exact hole 4490 closed.

**Structural cause (c): parallel name registries with no cross-check.** Compiler side: `units.rs` `GEOMETRY_FUNCTION_NAMES` `:21`, `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` `:212`, selector result-type map `:340-348`, `geometry_query_result_type` `:1258`, plus dynamics/constructor/fea-envelope/field-op families (`:838-1010`) and the signature files (`builtin_signatures.rs` 841 ln, `math_signatures.rs` 1826 ln, `joint_signatures.rs` 475 ln, `analysis_signatures.rs` 319 ln, `parse_signatures.rs` 150 ln, `relation_signatures.rs` 1416 ln; `signatures_common.rs` is an admitted half-finished dedup at 58 ln). Eval side: `geometry_ops.rs` `is_geometry_query_call` `:3856`, `is_geometry_consumer_call` `:3906` (hand-maintained name list, prose "Maintenance contract" `:3895-3901`, and a *documented in-list gap*: `angle` at `:3933-3941`), `TopologySelectorHelper` name map `:5314-5352`, `try_eval_topology_selector` `:6389`. Disjointness within the compiler is enforced by 32 hand-written `*_are_disjoint*` tests in units.rs; **cross-crate agreement is enforced by nothing**. The 19-family FunctionCall result-type ladder (`expr.rs:3006-3400`, precedence doc `:2920-2965`) consumes these families.

**Structural cause (d): MemberAccess if-chain.** `expr.rs:3403-4707`: sequential if-guards over receiver `compiled_obj.result_type` and member names, with documented cross-guard precedence ("This branch MUST fire before the OUTER MemberAccess…" `:3782`; "Placed BEFORE the no-member poison…" `:4313`, `:4347`) and a genuinely-diagnostic poison tail (`:4690-4706`, "member access not yet supported"). `Type` (reify-core `ty.rs:111`) has **39 variants** (37 at survey time; `Feature`, `Relation` since), no strum discriminants, no completeness test. The exhaustive-match pattern is already proven in this crate: `type_carries_type_param` (`type_compat.rs:438-499`, "intentionally exhaustive (no `_` wildcard)") and `substitute_type_params` (`type_resolution.rs:2009-2030`).

**Duplication (e): auto_free decl construction.** `let auto_free = param.default…extract_auto_free; let decl = if let Some(free) { ValueCellDecl{ kind: Auto{free}, … } } else { compile default + check + ValueCellDecl{ kind: Param, … } }` copied at entity.rs `:2024-2064` (Site 1, top-level param; priv-aware visibility, real solver_hints), entity.rs `:3264-3300` (Site 2, port-member param; `Visibility::Public`, `solver_hints: Vec::new()`), guards.rs `:413-446` (guarded param; `compile_expr_guarded`). Task 1333 (done) extracted `extract_auto_free` itself; the surrounding decl construction is the remaining copy-paste.

## 3. Resolved design decisions

1. **Trait type-args: reject now, design later** (Leo-ratified 2026-07-06; sole user, free move). Add a trait-with-args intercept arm in `resolve_type_expr_with_aliases_kinded` mirroring the 4603 structure arm's shape (placed adjacent to `:1704`), emitting `Severity::Error` with a **new `DiagnosticCode::TypeArgOnTrait`** (PRD mnemonic `E_TYPE_ARG_ON_TRAIT`, joining the TypeArgArity/TypeArgBound family in reify-core `diagnostics.rs:2173/:2196`), message naming the trait, the args as written, and the hint that trait type-arguments are not yet supported. Return `Some(Type::Error)` (poison, anti-cascade — the BareScalarType pattern at `:1770-1780`). **Breadcrumb comment at the arm names the deferred alternative** — extending `Type::Applied` to traits with bound/arity checking — citing **task #5024** (the generic-trait language-design PRD bookmark; `feedback_breadcrumb_design_alternatives_at_impl_site`). Test reuses the 4603 fixture pattern.
2. **Mul/Div: type correctly, do NOT blanket-reject** (Leo-ratified). Sequence: **inventory first** (β1 pins the runtime truth table as tests), then make `infer_binop_type` return the correct type for every runtime-supported combination and `expr.rs` emit a diagnostic for runtime-unsupported ones. New guard joins the existing operand-guard region (after `:1806`); **new `DiagnosticCode::ArithOperandKind`** (mnemonic `E_ArithOperandKind`, mirroring `CmpOperandKind`), message naming the op and both operand types.
3. **4490 gradualism holds**: `Type::Error` and `TypeParam` operands pass silently — already structurally guaranteed for arithmetic by the propagation block at `type_compat.rs:1541-1564` (4629 W5); the new guard must preserve it.
4. **Parity test with an explicit exemption ledger.** One table-driven static-vs-runtime parity test so the two tables cannot drift. Known honest exemptions are listed with rationale in the test, starting with: Int/Int division (runtime widens `Int|Real` by divisibility; static stays `Int` — changing integer-division statics is a language-semantics decision, out of scope). Every exemption row requires a comment; an uncommented divergence fails.
5. **Degenerate runtime arms classified unsupported.** `scale_components`'s broad guards (e.g. `(Tensor, non-Tensor)`) make Tensor×Vector produce nested tensor-of-vectors at runtime — shape garbage, not an intentional product. The static table treats aggregate×aggregate (except the intentional Transform family) as **diagnostic**; tightening the eval-side arms is future eval-side work (survey H1-P7b) and is **not filed here** (G4).
6. **Drift test placement**: new in-crate `#[cfg(test)]` module in reify-eval (new file + one-line `mod` hook in `lib.rs`) — reify-eval already depends on reify-compiler (Cargo.toml:15), and the eval-side predicates are `pub(crate)`. Probes the eval-side predicates/dispatchers with synthesized `FunctionCall` exprs per compiler-registered name; asserts family agreement both ways plus cross-crate coverage; documents the known `angle` gap (`:3933-3941`) as an expected-divergence ledger entry citing task 4952 α rather than silently baking it in.
7. **Full registry unification is Wave 3 — bookmark only** (δ stays deferred; no implementation now). The drift test is named as the registry's first migration validator.
8. **MemberAccess reshape is behavior-preserving**: one exhaustive `match &compiled_obj.result_type` dispatching to named per-kind handler fns, explicit **named tail arm** (not `_`) that keeps the existing poison fallback verbatim. Documented precedence comments are translated into arm order where type-directed; **name-directed pre-checks that span receiver kinds** (datum projection, purpose-subject reflection, cross-sub shapes) stay as an explicit documented pre-pass before the match OR are proven order-independent in the task — not silently reordered. Diagnostics must be byte-identical across the examples corpus.
9. **Type-variant completeness substrate**: `#[derive(strum::EnumDiscriminants)]` on reify-core `Type` (strum 0.26 already a workspace dep; reify-ir `GeometryOp` precedent at `geometry.rs:569`), fieldless discriminants with `EnumIter`/`EnumCount`. Completeness canary asserts every discriminant maps to a named MemberAccess handler class (or the named tail).
10. **auto_free dedup**: one shared helper (entity.rs-hosted, callable from guards.rs) parameterized by visibility, solver_hints, and a default-compile closure; the three sites' observable outputs stay identical.
11. **Diagnostic-code hygiene**: new codes get the full doc-comment shape (origin, canonical message form, distinct-from notes, PRD mnemonic) matching TypeArgArity's exemplar.

## 4. Out of scope (named)

- **Collapsing the 17 `resolve_*` entry points** in type_resolution.rs — assessed and rejected by the survey (legitimately distinct concerns; the apparent duplicate is an intentional default-args wrapper). Do not file.
- **The unified builtin-signature registry implementation** — Wave 3; bookmark δ only.
- **Eval-side binop-cascade table / arm tightening** (survey H1-P7b) — referenced by decision 5, owned by future eval-side work, not this PRD.
- **Generic-trait type-args language design** — task #5024's PRD slot.
- **Integer-division static-type change** — exemption-ledger entry, not a change.

## 5. Pre-conditions

All substrate verified 2026-07-06 against main `4d696e63cb`: trait-with-args syntax parses today (probe compiled; no novel grammar — **G3 N/A for syntax**); `DiagnosticCode` extension mechanics (reify-core diagnostics.rs); reify-compiler dev-deps on reify-expr + reify-eval (Cargo.toml:24-30) for the parity test; reify-eval hard-dep on reify-compiler for the drift test; strum workspace dep (Cargo.toml:77) + reify-ir discriminants precedent for ε2. No out-of-batch task dependencies.

## 6. Cross-PRD relationships (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| #5024 generic-trait design (bookmark, `docs/prds/generic-trait-type-args.md` unauthored) | produces breadcrumb | α's rejection arm + `// … deferred alternative: Type::Applied for traits — #5024` comment | this PRD emits; #5024 consumes/replaces the rejection when authored | queued (bookmark exists, deferred) |
| `docs/prds/v0_6/type-hygiene.md` (wave 1, done) | consumes precedent | 4490 gradualism contract, 4493 `check_builtin_arg_types` wiring point, CmpOperandKind naming | wave 1 (landed) | wired |
| Wave-3 registry PRD (unauthored; slot = task δ) | produces validator | γ's drift test is the registry's first migration validator; δ's description names it | δ bookmark holds the slot | queued-deferred |
| eval-side numeric-promotion table (survey H1-P7b; eval-side PRDs) | references only | β1's runtime inventory tests document the cascade's intentional arms | eval-side program | not filed here |

No new contested-ownership pair (checked against the overlay's three known pairs).

## 7. Contracts (H)

### 7.1 Trait-with-args rejection (α)

For a `Named { name, type_args }` type expression where `name ∈ trait_names` and `type_args` non-empty, reached in any position `resolve_type_expr_with_aliases_kinded` serves: emit exactly one `E_TYPE_ARG_ON_TRAIT` error naming the trait and arg count/spelling, return `Some(Type::Error)`. Empty `type_args` keeps today's `Type::TraitObject` resolution byte-identical. Structures keep the 4603 `Type::Applied` path. Precedence: the new arm sits with the structure arm, before simple-name resolution, so it fires regardless of same-name shadows later in the fallthrough.

### 7.2 Static/runtime operator alignment (β; INV-COMP-3)

For op ∈ {Mul, Div} and static operand kinds (K_L, K_R) with `Type::Error`/`TypeParam` excluded (decision 3):
- If the runtime cascade has an **intentional** arm for the corresponding value kinds → `infer_binop_type` returns the type of the runtime result: Scalar⊗Scalar dimension algebra (`ld.mul(rd)`/`ld.div(rd)`, exists); scale ops return the **aggregate** type with its quantity slot recursed through the same rule (Vector3<Length> * dimensionless → Vector3<Length>; Vector3<Length> / Scalar<Time> → Vector3<Velocity-dim>); Complex arms combine dimensions; Transform×Vector→Vector, Transform×Point→Point, Transform×Transform→Transform; Div is non-commutative (Scalar/Vector is NOT a scale op).
- Else → `E_ArithOperandKind` diagnostic at the expr.rs guard region + poison per the anti-cascade convention.
- The parity test (β3) enumerates kind pairs, builds representative `Value`s, evaluates a `CompiledExpr::binop` through public `eval_expr`, and asserts: runtime non-Undef ⇒ static type kind matches the result value's kind; runtime structurally-Undef (excluding data-driven Undef like divide-by-zero) ⇒ the static layer rejects. Divergences allowed only via the commented exemption ledger (decision 4).

### 7.3 Cross-registry drift (γ; INV-COMP-2 interim)

For each compiler-side name family with an eval-side consumer map, the test asserts set-level agreement in both directions (compiler-registered name ⇒ eval dispatcher recognizes it; eval-recognized name ⇒ some compiler family claims it), plus pairwise disjointness across crates for the families the 32 in-crate units.rs tests do not span. Known intentional divergences live in one commented ledger in the test. Negative row: a synthetic name inserted in exactly one registry must fail the assertion (proven in the task by temporary mutation, kept as a doc-comment recipe).

### 7.4 MemberAccess dispatch (ε; INV-COMP-1)

`match &compiled_obj.result_type` with one arm per receiver-type class, each delegating to a named `fn member_access_on_<kind>(…)`; cross-kind name-directed pre-checks documented as a pre-pass; the tail is the existing named poison fallback. The match has **no `_` arm**. Byte-identical diagnostics on the examples corpus + existing test suite green is the behavior-preservation gate; the ε2 canary makes variant addition loud.

## 8. Boundary-test sketch (two-way)

| # | Scenario | Pre (verified today) | Post |
|---|---|---|---|
| 1 | `param m : SpecLike<Foo>` (trait, 1 arg) | exit 0, silent, `Foo` dropped | `E_TYPE_ARG_ON_TRAIT` naming `SpecLike`/`Foo`; exit ≠ 0 |
| 2 | `param m : SpecLike` (trait, bare) | resolves `TraitObject` | unchanged (byte-identical) |
| 3 | `Coupling<Prismatic>` (structure w/ args) | `Type::Applied` + 4603 checks | unchanged |
| 4 | `let v = a * a`, a : Vector3<Length> | silent; statically Int; evals Undef | compile error `E_ArithOperandKind` |
| 5 | `let s = a * 2.0; constraint s > 0` | s statically Scalar; guard defeated; permanently indeterminate | s : Vector3<Length>; `s > 0` → 4490 `CmpOperandKind` error with fixit |
| 6 | `let q = w / 2.0`, w : Vector3<Force> | q statically Scalar | q : Vector3<Force>; type-assertion test pins it |
| 7 | `2.0 / a` (Scalar / Vector) | statically Scalar; evals Undef | compile error (Div non-commutative) |
| 8 | TypeParam-typed operand in `*` | passes (4629 W5) | still passes silently (gradualism) |
| 9 | `t1 * t2` Transform composition | statically Int; evals Transform | statically Transform |
| 10 | name added to `is_geometry_consumer_call` only | nothing trips | γ drift test RED |
| 11 | new `Type` variant added | MemberAccess chain silently falls to poison tail | ε1 match forces compile-time decision; ε2 canary counts it |
| 12 | examples corpus + full test suite through ε1/ζ | current diagnostics | byte-identical |

## 9. Decomposition plan

Real IDs at decompose; `metadata.files` tight-or-empty. **Spine:** β1 → β2 → β3; β2 → ε1 → ε2; γ → δ(bookmark); all-but-δ → λ.

- **α — trait type-arg rejection arm + E_TYPE_ARG_ON_TRAIT + #5024 breadcrumb.** INV-COMP-1. Files: `crates/reify-compiler/src/type_resolution.rs`, `crates/reify-core/src/diagnostics.rs`. Deps: none. Priority: high. Signal: `reify check` on the committed probe fixture (`tests/prd-gate/fixtures/compiler_type_hygiene_trait_args_silent_accept.ri`) emits `E_TYPE_ARG_ON_TRAIT` naming trait+arg and exits non-zero (today: exit 0 silent — probe-verified); bare-trait and structure-with-args fixtures byte-identical.
- **β1 — runtime Mul/Div truth-table inventory tests.** INV-COMP-3 (producer). New file `crates/reify-expr/tests/mul_div_runtime_truth_table.rs`: table-driven over value-kind pairs via public `eval_expr`, pinning every intentional arm of eval_mul/eval_div (§2 inventory) and the structural-Undef set; classifies degenerate scale_components shapes (decision 5) with comments. Deps: none. Priority: high. Signal (intermediate): unlocks β2/β3 — the pinned table is the source of truth the static table is written against; test runs in CI.
- **β2 — static Mul/Div table fix + E_ArithOperandKind guard.** INV-COMP-3. `infer_binop_type` Mul/Div arms per §7.2 + guard in the expr.rs operand-guard region + `ArithOperandKind` code. Files: `crates/reify-compiler/src/type_compat.rs`, `crates/reify-compiler/src/expr.rs`, `crates/reify-core/src/diagnostics.rs`. Deps: β1. Priority: high. Signal: boundary rows 4–9 — `reify check` on the committed probe fixtures flips from silent to error (rows 4/7) and from guard-defeat to 4490 fixit (row 5); a runtime-legal scale compiles with the correct type (row 6, type-assertion test).
- **β3 — static-vs-runtime parity test + exemption ledger.** INV-COMP-3 (enforcement flip: proposed → enforced(test)). New file `crates/reify-compiler/tests/mul_div_static_runtime_parity.rs` (dev-deps reify-expr, present). Deps: β1, β2. Priority: high. Signal: parity test in CI; any uncommented static/runtime divergence for Mul/Div fails; Int/Int-div exemption documented.
- **γ — cross-registry drift test.** INV-COMP-2 interim (enforced(test)). New in-crate module `crates/reify-eval/src/registry_drift_tests.rs` + one-line mod hook in `crates/reify-eval/src/lib.rs`, per §7.3. Deps: none. Priority: medium. Signal: negative row 10 — a name present in exactly one registry fails CI (was: nothing trips; prose contract only); the `angle` gap is ledgered, citing 4952 α.
- **δ — [BOOKMARK] Wave-3 unified builtin-signature registry.** Stays **deferred** (excluded from commit_planning; `preferences_bookmark_task_pattern`). Names γ's drift test as its first migration validator and the 32 units.rs disjointness tests + signatures_common.rs as the surface to be subsumed family-by-family. Deps: γ. Priority: low.
- **ε1 — MemberAccess exhaustive-match reshape.** INV-COMP-1. Per §7.4/decision 8. Files: `crates/reify-compiler/src/expr.rs`. Deps: β2 (same-file serialization on expr.rs — deliberate ordering edge, not a semantic dep). Priority: medium. Signal: examples corpus + full compiler test suite byte-identical diagnostics (row 12); the match compiles with no `_` arm (reviewable in the diff; canary follows in ε2).
- **ε2 — Type strum discriminants + MemberAccess completeness canary.** INV-COMP-1 (enforcement flip for the completeness half). Files: `crates/reify-core/src/ty.rs`, `crates/reify-core/Cargo.toml`, `crates/reify-compiler/src/expr.rs` (canary test module). Deps: ε1. Priority: medium. Signal: row 11 — the canary iterates `TypeDiscriminants` and asserts every variant is claimed by a named handler class or the named tail; adding a variant without classifying it fails CI (was: nothing trips).
- **ζ — auto_free decl-construction dedup.** Decision 10. Files: `crates/reify-compiler/src/entity.rs`, `crates/reify-compiler/src/guards.rs`. Deps: none. Priority: medium. Signal: `reify eval` output on the auto(free)-exercising examples/tests byte-identical (existing boundary2_producer + auto-free suites green); the three sites route through one helper (diff-reviewable).
- **λ — integration gate.** The §8 boundary table as committed tests + one CI fixture pass: probe fixtures under `tests/prd-gate/fixtures/compiler_type_hygiene_*.ri` exercised (rows 1–7, 9), drift-test negative recipe documented (row 10), corpus byte-identity re-asserted after all landings (row 12). Deps: α, β3, γ, ε2, ζ. Priority: medium. Signal: every §8 row green in CI on one commit.

## 10. Open questions (tactical) + conservative resolutions made in this non-interactive session

1. **Exact diagnostic message wording** for E_TYPE_ARG_ON_TRAIT / E_ArithOperandKind (codes and family placement are decided; prose is the implementer's). Decide in α/β2.
2. **Matrix (`Value::Matrix`) arms**: no intentional eval arms exist → statically diagnostic per §7.2. If β1's inventory finds a real consumer expecting Matrix scaling, weaken to correct-typing for that combo in β2 and note it for H1-P7b. Decide in β1.
3. **Handler-fn granularity in ε1** (one fn per Type variant vs per handler-class with grouped arms). Either preserves the contract; pick for diff-reviewability. Decide in ε1.
4. **Conservative choices made without Leo** (brief was silent): (i) priorities — α/β* high (user-facing latent-bug fixes, survey Wave 1), γ/ε*/ζ/λ medium, δ low; (ii) the ε1←β2 expr.rs serialization edge (avoids concurrent big-diff merges in the repo's #1 contention file); (iii) new diagnostic code names `TypeArgOnTrait`/`ArithOperandKind` following the closest existing family conventions; (iv) drift-test in-crate module placement (forced by `pub(crate)` predicates + keeping geometry_ops.rs unlocked); (v) Int/Int-div exemption (decision 4) rather than changing integer-division static semantics.
