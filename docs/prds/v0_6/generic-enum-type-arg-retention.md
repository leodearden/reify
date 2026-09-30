# PRD: Generic-Enum Type-Argument Retention (D1 of generic-data-carrying-enums, compile-time half — scope S1)

**Status:** ACTIVE — authored 2026-09-29 in the esc-6020-11 session on Leo's ruling ("/prd reverse D1"); revised the same day after an adversarial critic pass (three blockers, eight majors, all applied); forks R1–R5 **ruled by Leo 2026-09-29** (§6); decomposed 2026-09-29 as tasks #8012–#8023 (per-leaf IDs in §9).

**Code anchors** verified against main `300910857b` (2026-09-29). Main moves fast — cite-by-symbol; re-locate lines at implementation time. **Probe binaries:** `target/debug/reify` built 2026-09-25 16:52 and `target/release/reify` built 2026-09-01. Nine of the cited source files changed between the debug build and `300910857b` (`type_compat.rs`, `overload.rs`, `engine_eval.rs`, `expr.rs`, `conformance/mod.rs`, `entity.rs`, `ty.rs`, `variant_construct.rs` via the 5889/6553/7107 merges and the 5506 back-merge); the critic seat read those diffs and judged them inert for every fixture here, but they were **not re-measured** on a HEAD build. The release binary predates #5306 (ctor-conformance severity flipped to Error, landed 2026-09-29 `e9a73c025a`), so the two binaries **disagree on conformance probes** (`param b : Result<Force,String> = 5mm` is rejected by debug, accepted by release) and on one eval path (`getar_wildcard_headed_arg_silent.ri` evaluates to `Result::Ok` on debug, `undef` on release). **Every POST-state verification in this PRD must run on a rebuilt binary**, and the probe harness (`scripts/prd-capability-check.py`, which prefers `target/release/reify` and honours `REIFY_BIN`) must be pointed at it.

## §0 — What this PRD changes about D1

Decision D1 (fork F-Mono) of `docs/prds/v0_6/generic-data-carrying-enums.md` reads: "type args are resolved/**checked at compile time** and **erased before eval**". The implementation that shipped erases **earlier** than that clause says: a constructed variant's static type is the bare `Type::Enum(name)` from the moment `compile_variant_construct` returns, so nothing downstream at compile time can check its arguments. This PRD (scope **S1**) makes the compiler honour D1's own compile-time clause — a constructed generic variant keeps its inferred arguments in its **static** type — and leaves D1's runtime clause exactly as shipped: `Value::Enum` carries no type-argument tag, the content hash is unchanged, both evaluators stay type-arg-agnostic. Leo's instruction that produced this PRD was "reverse D1"; in substance it is a reversal of the *as-implemented* erasure point, not of D1's text. The superseded PRD's §7.3 note names "a future PRD that reverses erasure (F-Mono-b / runtime type-arg reflection)" as the owner of `E_ENUM_TYPE_ARG_UNRESOLVED`; that describes S2 (runtime reflection), which this PRD explicitly does not do (§10). Fork R4 below decides whether S1 mints the diagnostic anyway.

The superseded PRD is frozen `SHIPPED`; leaf α edits only its LIVE header (a pointer to this PRD), never the frozen body.

## §1 — Goal, consumers, user-observable surface (G1)

**Goal.** A constructed generic-enum variant (`Ok { value: 5mm }`, `Leaf { value: 1mm }`) carries its inferred type arguments in its static type (`Result<Length, ?E>`, `Tree<Length>`) instead of collapsing to `Enum("Result")` / `Enum("Tree")`, so the compiler can compare, unify and diagnose generic-enum values with the precision it already applies to generic structures (`Coupling<Prismatic>`) and to the intrinsic `Option<T>`.

**Consumers, each named and observable.** Fixtures live under `tests/prd-gate/fixtures/`; every header records the 2026-09-29 measurement on both binaries.

1. **The `/prd E` milestone (Option as a prelude enum)**, filed as a MILESTONE task depending on this PRD's leaves (esc-6020-11 ruling). Precondition: `Applied{Option,[Length]}` and `Applied{Option,[Force]}` distinguishable at compile time the way `Type::Option(Length)` / `Type::Option(Force)` are today. Without S1 a prelude `enum Option<T>` would silently accept `Option<Length> = some(3N)`.
2. **Four soundness holes closed** (each a CLI diagnostic that is absent today):
   - **S-1** `getar_fallback_type_mismatch_silent.ri` — `let v : String = fallback(Ok { value: 5mm }, "y")` checks clean and evaluates to `0.005 m`: static type `String`, runtime `Length`.
   - **S-2** `getar_result_arg_mismatch_assign_silent.ri` — `param b : Result<Force, String> = a` with `a : Result<Length, String>` checks clean; a match binder typed `Force` holds `0.005 m`.
   - **S-3** `getar_tree_force_leaf_length_silent.ri` — `param t : Tree<Force> = Node { left: Leaf { value: 1mm }, … }` checks clean.
   - **S-4 (pre-existing, tier-3 wildcard)** `getar_wildcard_headed_arg_silent.ri` — `fl(or_else(Ok{..}, Ok{..}))` with `fn fl(x: Length)` checks clean and passes a `Result` as a `Length`; `getar_wildcard_placeholder_binder_silent.ri` — an untyped `Err` binder is accepted by a `Bool` parameter. Both go through `slot_matches_wildcard_tier`'s `type_carries_type_param(arg_ty)` disjunct, which S1 would otherwise widen to every partly-bound construction.
   Plus the **4031 recursive conflict** `getar_recursive_conflict_silent.ri` (`Node { left: Leaf{1mm}, right: Leaf{1N} }` checks clean; the superseded PRD §7.3 recorded it as infeasible under erasure).
3. **One debug-profile panic closed (P-1)** — `getar_match_over_let_construct_panics.ri`, `getar_match_over_annotated_let_panics.ri`, `getar_match_err_first_arm_panics.ri`, `getar_match_err_subject_ok_first_panics.ri`: a `match` over a `let`-bound constructed variant panics `reify check`/`reify eval` in **debug** builds ("unrepresentable cell_type … TypeParam(\"T\")", `is_representable_cell_type` in `crates/reify-eval/src/engine_eval.rs`). In **release** builds all four exit 0 with the **correct runtime value**; the defect is a wrong **static** cell type that only the debug profile checks — so it is not a runtime soundness hole, but every debug-profile consumer (nextest, the GUI dev build, LSP hover typing) sees it. The first two are closed by δ alone; the last two need the R5 rule (ε).
4. **One usability gap closed (U-1)** `getar_fn_param_rejects_construct.ri` — `fn f(r: Result<Length, String>)` called as `f(Ok { value: 5mm })` fails "no matching overload for f(Enum(Result))".
5. **Two pre-existing `Applied`-handling defects closed** (not S1-induced, but S1 makes them reachable from every construction): `getar_decl_match_applied_param_rejected.ri` — a decl-form `match` over an annotated `param r : Result<Length,String>` is refused "expected an enum" today (ε2); and the param-override kind check `value_type_kind_matches` refuses `Applied` for `Value::Enum` (θ, read from code).
6. **`result-and-fallback.md` Layer B combinators** (landed): `E_FALLBACK_TYPE` fires only for annotated subjects today (`result_fallback_resolution_tests.rs` needs `param r : Result<Length,String>`); after γ+δ it fires for an unannotated `fallback(Ok { value: 5mm }, "y")`.

No mechanism here is a producer without one of these consumers. (Task #5992, the compiler/eval overload-parity test, is **not** a consumer: #5689 already hoisted the three tier predicates into `crates/reify-core/src/overload.rs`, so β's change reaches all three callers — `resolve_function_overload`, `find_matching_compiled_function`, and the `TraitMethodCall` arm in `expr.rs` — through one predicate; #5992's remaining subject is the policy layers above it.)

## §2 — Background: what shipped, and why its premise is stale

**As shipped.** `Type::Enum(String)` has no argument slot. A generic-enum **annotation** resolves to `Type::Applied { name, args }` (`resolve_enum_type_with_args`, `crates/reify-compiler/src/type_resolution.rs`). A **constructed** variant's `result_type` is the bare `Type::Enum(name)` (`compile_variant_construct`, `crates/reify-compiler/src/variant_construct.rs`, all three exits) even though `final_subst` at the success exit already holds the pinned or inferred arguments. The two spellings are reconciled by a **name-only** oracle, `base_enum_name` / `enum_payload_compatible` (`crates/reify-compiler/src/type_compat.rs`), whose doc says it "does not re-check args". `heads_unifiable` (`crates/reify-core/src/overload.rs`) has the matching erased-subject arm (`Applied{name}` param vs `Enum(name)` arg). `unify` treats a bare `TypeParam` arg as provisional ("erased side yields") but **hard-conflicts two different `TypeParam` bindings** (pinned by `unify_two_distinct_erased_params_still_conflict`). Runtime shape pinned by `crates/reify-expr/tests/generic_enum_erasure_tests.rs` (reads the `Value` only); compile-time erasure pinned by two `Type::Enum("Result")` assertions in `crates/reify-compiler/tests/harness_result_annotation/result_prelude_enum_tests.rs`.

**The "match the structure side" premise was true when written and is stale now.** D1 was verified 2026-05-27, when generic structure references lost their arguments. `Type::Applied` arrived 2026-06-16 (task 4602 β); since then structures keep arguments at compile time — annotations are `Applied` (`ty.rs` doc: "compile-time only … erased before evaluation … `Applied{"C",[Prismatic]} != Applied{"C",[Revolute]}`"), builtin producers return `Type::applied("Coupling", …)` (`joint_signatures.rs`), `substitute_type_params` rebuilds `Applied` for generic-fn returns. Structures erase in two places only: a user constructor expression `Foo(...)` is typed `StructureRef(name)` (call syntax has no argument slot), and `StructureInstanceData` carries no args. Generic enums erase **earlier**, at construction, where the arguments are inferred and then dropped. S1 brings enum construction to parity with structure annotations and generic-fn returns; it does not depart from the structure side.

**Why it bites.** Every post-construction comparison is name-only or equality-only: conformance's enum branch (`crates/reify-compiler/src/conformance/mod.rs`, `base_enum_name(arg) != Some(param_enum)`), `implicitly_converts_to`'s `from == to`, the exact overload tier, `heads_unifiable`'s catch-all. A let's `cell_type` is always the initializer's `result_type` (`entity.rs`, the let lowering's `let cell_type = compiled_expr.result_type.clone()`), the annotation being only an expected-type hint, so even an annotated `let r : Result<Length,String> = Ok{..}` is erased; only a `param` keeps its declared `Applied`. P-1's path: erased let → empty binder substitution → binder typed `TypeParam("T")` → match result = first arm's body type → `let v` typed `TypeParam("T")` → debug assert. And the tier-3 wildcard hole (S-4) is independent of D1: `slot_matches_wildcard_tier` = `trait-object param || (generic && param carries type/dim param) || param == arg || type_carries_type_param(arg)`; tier 3 is the **only** gate a candidate must pass (tier 2 narrows only the param-side disjunct), so any arg whose type contains a `TypeParam` anywhere — today the leaky `Applied{Result,[T,E]}` from a combinator chain, or an untyped match binder — matches every candidate including concrete non-enum params.

**Measured 2026-09-29** (debug = `target/debug/reify` 2026-09-25; release = `target/release/reify` 2026-09-01):

| Fixture (`tests/prd-gate/fixtures/`) | debug `check` / `eval` | release `check` / `eval` | Class |
|---|---|---|---|
| `getar_fallback_type_mismatch_silent.ri` | 0 / 0, `W.v = 0.005 m` | same | S-1 |
| `getar_result_arg_mismatch_assign_silent.ri` | 0 / 0, `W.n = 0.005 m` (typed Force) | same | S-2 |
| `getar_tree_force_leaf_length_silent.ri` | 0 / 0 | same | S-3 |
| `getar_wildcard_headed_arg_silent.ri` | 0 / 0, `W.v = Result::Ok` | 0 / 0, `W.v = undef` | S-4 |
| `getar_wildcard_placeholder_binder_silent.ri` | 0 / 0, `W.v = false` | same | S-4 (binder) |
| `getar_recursive_conflict_silent.ri` | 0 / 0 | same | 4031 |
| `getar_fn_param_rejects_construct.ri` | 1 / 1, no matching overload | same | U-1 |
| `getar_match_over_let_construct_panics.ri` | 101 / 101 panic | 0 / 0, `W.v = 0.005 m` | P-1 |
| `getar_match_over_annotated_let_panics.ri` | 101 / 101 panic | 0 / 0, `W.v = 0.005 m` | P-1 (annotated) |
| `getar_match_err_first_arm_panics.ri` | 101 / 101 panic | 0 / 0, `W.v = "ok"` | P-1 (R5) |
| `getar_match_err_subject_ok_first_panics.ri` | 101 / 101 panic | 0 / 0, `W.v = 0.001 m` | P-1 (R5) |
| `getar_bare_result_annotation_match_panics.ri` | 101 / 101 panic | 0 / 0, `W.v = 0.005 m` | P-1 via bare annotation (R2) |
| `getar_decl_match_applied_param_rejected.ri` | 1 / 1, "expected an enum" | same | ε2 (pre-existing) |
| `getar_decl_match_let_construct_ok.ri` | 0 / 0, `Bolt.r = Result::Ok` | same | ε2 regression guard |
| `getar_let_annotated_result_arg_mismatch_silent.ri` | 0 / 0 | same | §10 out of scope |

## §3 — Current chain (sites S1 touches; cite by symbol)

**Producer sites of a bare `Type::Enum(name)` for a generic enum value:** `compile_variant_construct` (three exits; `final_subst` available at the success exit); the `EnumAccess` compile arm in `crates/reify-compiler/src/expr.rs` (`E.V` unit-variant access — note the `.` spelling; `E::V` is parsed as a trait path and fails "trait not found") — ignores `type_params`, consults no expected type; `resolve_enum_type_with_args` bare-name and arity-mismatch paths; `crates/reify-builtins/src/registry.rs` `parse_length_r` registered `Const(Type::Enum("Result"))` (its own comment says the headless registration exists so the eval-time `value_type_kind_matches` guard passes — the θ↔η coupling); runtime `Value::try_infer_type` (S2, out of scope).

**Comparison / consumer sites:**
- `base_enum_name`, `enum_payload_compatible` (`type_compat.rs`) — name-only.
- conformance enum branch (`conformance/mod.rs`) — name-only via `base_enum_name`.
- `implicitly_converts_to` (`from == to`), `resolve_function_overload` exact tier, `heads_unifiable` catch-all — structural equality.
- `unify` (`type_compat.rs`) — Applied/Applied recurses; **TypeParam arm hard-conflicts two distinct `TypeParam` bindings**. **S1 changes this arm** (§4 step 3).
- `slot_matches_wildcard_tier` / `slot_matches_head_tier` (`overload.rs`) — three consumers: `resolve_function_overload`, `find_matching_compiled_function` (`reify-expr`), the `TraitMethodCall` arm in `expr.rs`. **S1 narrows tier 3** (§4 step 4).
- match binder substitution (`expr.rs` Match branch, `binder_subst` from `Type::Applied`) — already correct for `Applied`; task **#6020**'s plan extracts it into one resolver `match_discriminant_enum` shared with the exhaustiveness lookup (§8).
- match result type = first arm's body type (`expr.rs` Match branch).
- decl-form `MatchArmDeclGroup` discriminant check (`entity.rs`, "match-arm discriminant … expected an enum") — accepts `Type::Enum` only.
- `value_type_kind_matches` (`crates/reify-eval/src/lib.rs`) — `Value::Enum` accepts `Type::Enum(_)` only; callers: engine-admin param override and `registry_parity_tests.rs::classify` (which is why `parse_length_r` is registered headless).
- `check_let_annotation_type` (`entity.rs`) returns early for non-scalar declared types — the let form of S-2 stays silent (§10).
- GUI `structural_fingerprint` (`gui/src-tauri/src/engine.rs`) hashes `cell.cell_type.to_string()` — `Type`'s `Display` must be stable across compiles for placeholder-typed cells (R1).

**Erasure pins:** `generic_enum_erasure_tests.rs` (runtime; stays green); `result_prelude_enum_tests.rs` two `Type::Enum("Result")` assertions (δ flips); `registry_seed_result_types.rs` and `parse_length_signatures.rs` doc-and-assert pins plus the in-crate test in `registry.rs` (η flips; `registry_parity_tests.rs` is an Undef sweep, not a pin). Prose describing erasure in `enum_generic_construction_inference_tests.rs` and `struct_ctor_field_conformance_tests.rs` (comments only).

## §4 — Sketch of approach (S1)

1. **Construction emits `Applied`.** `compile_variant_construct`'s success exit returns `Type::Applied { name, args }` with `args[i] = final_subst[type_params[i]]` when bound, else the **R1 placeholder**. Non-generic enums keep `Type::Enum(name)` (`Applied`'s "non-empty args" invariant preserved; every non-generic DCE golden byte-identical). The two failure exits emit the same shape.
2. **One args-aware enum-compat oracle.** `base_enum_name`'s name-only tolerance becomes one predicate that returns the base name and, when BOTH sides are `Applied`, requires arity equality and per-arg compatibility under the placeholder rule; `enum_payload_compatible` and the conformance enum branch route through it (SPOT). A bare `Enum(n)` on either side still matches by name (D-3) — retained deliberately for S2-era runtime inference and any residual bare producer; under R2(b) bare *annotations* no longer produce one.
3. **`unify` placeholder rule (R1-coupled).** A placeholder `TypeParam` unifies with anything, **including another placeholder**, and never records a conflict; user-declared `TypeParam`s keep today's conflict rule (the `unify_two_distinct_erased_params_still_conflict` pin stays green). Without this, `or_else(Ok{value:1mm}, Ok{value:2mm})` — clean today — would bind `E` to the first construction's placeholder and then conflict with the second's, and nested children in `compile_variant_construct` would conflict the same way.
4. **Tier-3 narrowing (β).** In `slot_matches_wildcard_tier` the arg-side disjunct `type_carries_type_param(arg_ty)` is split: a **bare** user `TypeParam` arg keeps the D4 behaviour (pinned by `overload_bare_type_param_arg_still_resolves` and its eval twin); a **headed** arg that carries a type param or placeholder (`Applied`, `List`, …) matches only when `heads_unifiable(param_ty, arg_ty)`; a **bare placeholder** arg matches only params that themselves carry a type param (generic candidates), never a concrete param. This closes S-4 today and keeps δ from widening it. It is one edit, reaching all three consumers.
5. **Placeholder rule at the equality sites.** `implicitly_converts_to`, the exact tier and `heads_unifiable`'s catch-all learn: a placeholder arg is compatible with any type in the same slot. This is what makes U-1 resolve (`Applied{Result,[Length, ?E]}` against `Applied{Result,[Length,String]}`).
6. **Match result-type rule (R5).** No longer the first arm's body type unconditionally.
7. **Runtime unchanged (INV-1).** `Value::Enum`, content hash (`INV-5`), `Ord`/`Eq`, both evaluators' match arms untouched; `generic_enum_erasure_tests.rs` is the boundary test.
8. **Structure side untouched (INV-2).** `Foo(...)` keeps `StructureRef`; `Applied`↔`StructureRef` unification keeps its documented β posture.

## §5 — Resolved design decisions

- **D-1 Scope S1, not S2.** No runtime consumer of type args exists (eval overload selection reads `CompiledExpr.result_type`; the GUI reads `cell_type`; dual-eval reads variant + payload; content hash covers name, variant, payload; no runtime reader of `StructureInstance` args either). S2 would add a field to `Value::Enum` (~230 non-test sites), redefine hash/Eq/Ord and shift cache identity, for nothing in tree. Out (§10).
- **D-2 Reuse `Type::Applied`; no new applied-enum `Type` variant.** `Applied` is already the annotation carrier for enums and structures; a new variant touches every exhaustive `Type` match (the no-wildcard policy in `type_compat.rs`, `strum::EnumCount` in `ty.rs`).
- **D-3 Name-only tolerance survives for a bare `Enum(n)` value type.** Needed for S2-era runtime inference and any producer δ does not reach. **Hazard, recorded:** the tolerance would also silently paper over a *literal*-path `Applied` versus a runtime-`VariantCtor` `Enum(name)` inconsistency if #7456 landed its node with the bare type (its WORK item 1 says exactly that) — see D-8.
- **D-4 Ordering: β, γ and ε2 before δ.** Once δ emits `Applied`, every equality site that has not learned the placeholder rule rejects (β, γ), and the decl-form match refuses let-bound constructions it accepts today (ε2, `getar_decl_match_let_construct_ok.ri`). Hard `add_dependency` edges, not prose.
- **D-5 Non-generic enums byte-identical.** `Applied` only for `type_params` non-empty; the `Shape` goldens in `crates/reify-compiler/tests/harness_patterns/variant_construction_check_tests.rs` unchanged.
- **D-6 The runtime erasure test is the boundary test.** `generic_enum_erasure_tests.rs` is not edited. Compile-time pins asserting `Type::Enum("Result")` on a constructed value pin the defect and are flipped in δ.
- **D-7 `Option` stays intrinsic here.** The `/prd E` milestone owns that migration and consumes this PRD.
- **D-8 #7456 (runtime `VariantCtor`) shared-file seam.** Its WORK item 1 specifies "result type `Type::Enum(name)`" for the `VariantCtor` node. That must become the same `Applied` δ emits, or D-3's tolerance hides the inconsistency silently. **The lead stamps #7456's task text with this requirement now** (before either lands), and whichever lands second re-checks against the other's diff.
- **D-10 Fork rulings (Leo, 2026-09-29, esc-6020-11 session).** R1(c) reserved-prefix placeholder `__unbound_<P>` + the `unify` no-conflict rule; R2(b) bare annotations rejected via the arity diagnostic unless defaults fill every parameter; R3 defaults fill annotation positions only; R4 no mint here; R5 first placeholder-free arm. Full text in §6.
- **D-9 θ and ε2 are pre-existing `Applied`-handling defects**, reachable today through annotated params; S1 does not cause them but makes them reachable from every construction, so they are in scope and ordered accordingly (ε2 before δ; θ after α, before η).

## §6 — DESIGN FORKS — ALL RESOLVED (Leo, 2026-09-29, esc-6020-11 session)

> Each fork records the ruled resolution first, then the evidence and the critic's counter-evidence against the first draft's default (kept as provenance). **R1 and R5 are coupled**: R5's "first arm whose type carries no placeholder" needs a predicate that tells a placeholder from a user type parameter (`x: T` inside `fn f<T>`), which only a distinguishable placeholder representation provides.

### R1 — Representation of an UNBOUND type argument on a constructed value — **RESOLVED (Leo, 2026-09-29, esc-6020-11 session)**: option (c) — a `TypeParam` with the reserved prefix `__unbound_<P>` (precedent `AUTO_TYPE_PARAM_PLACEHOLDER_PREFIX`), plus the `unify` rule "a placeholder unifies with anything, including another placeholder, and never conflicts; user-declared type parameters keep today's conflict rule".

- **(a) `TypeParam("E")`, the enum's own param name.** Zero machinery. **Rejected by evidence:** `TypeParam` is an unscoped string namespace shared with fn generics and auto placeholders; `substitute_type_params` passes unknown names through; a leaked `E` is capturable by a caller's own `E`; and `type_carries_type_param` cannot tell a placeholder from a user param, so neither β nor R5 can be written against it.
- **(b, first draft) fresh site-unique placeholder names.** **Rejected by evidence:** `unify` hard-conflicts two distinct `TypeParam` bindings (`unify_two_distinct_erased_params_still_conflict`), so `or_else(Ok{value:1mm}, Ok{value:2mm})` — clean today — would bind `E` to `?E@1` then meet `?E@2` and emit a spurious conflict; nested children in `compile_variant_construct` likewise. And `gui/src-tauri/src/engine.rs` `structural_fingerprint` hashes `cell_type.to_string()`, so a site counter in the `Display` would change unrelated cells' identity between compiles.
- **(c, recommended) `TypeParam` with a RESERVED PREFIX, e.g. `__unbound_E`** — the in-tree precedent is `AUTO_TYPE_PARAM_PLACEHOLDER_PREFIX = "__auto_"` in `entity.rs`, with the `AutoTypeParamReservedPrefix` rejection at user declaration sites keeping the namespaces disjoint. A predicate `is_unbound_placeholder(&Type)` then serves β, R5 and the oracle. Same name at every site (no counter), so `Display` is stable (`?E`) and the fingerprint is unaffected. **Paired with the `unify` rule** (§4 step 3): placeholder unifies with anything including another placeholder, never conflicts; user params unchanged. Every existing provisional-binding rule still applies because it is still a `TypeParam`.
- **(d) a new `Type::Unbound { enum, param }` variant.** Cleanest, but touches every exhaustive `Type` match — the cost D-2 declines.

### R2 — Meaning of a bare generic-enum annotation `param r : Result` — **RESOLVED (Leo, 2026-09-29, esc-6020-11 session)**: option (b) — reject with the existing arity diagnostic ("enum `Result` expects 2 type arguments, found 0"), EXCEPT where declared defaults fill every parameter (R3). Leaf α2 is therefore a real leaf.

- **(a, first draft) keep legal, all args unbound.** Counter-evidence: it preserves a third P-1 path — `getar_bare_result_annotation_match_panics.ri` panics debug today and would survive S1 — and the claimed zero-churn advantage is not exclusive: the tracked corpus has **zero** bare generic-enum annotations (measured, `git grep -E 'param [a-z_]+ *: *(Result|Tree|Pair|Either|Maybe) *(=|$)' -- '*.ri'`).
- **(b, recommended) reject** — the arity-mismatch path in `resolve_enum_type_with_args` already diagnoses `Result<Length>`; the zero-arg path is the odd exception. Zero corpus churn (measured), kills the third P-1 path, and removes one producer of a bare `Enum(n)` so D-3's tolerance is needed for fewer shapes. **Exception (R3):** an enum whose every type parameter declares a default may be written bare; the defaults fill the annotation.
- Structures answer `sub c : Coupling` "accept"; the asymmetry is acknowledged and is what R3 addresses.

### R3 — Do declared type-parameter defaults fill unbound args? — **RESOLVED (Leo, 2026-09-29, esc-6020-11 session)**: yes, in ANNOTATION position only; construction sites keep the R1 placeholder.

- `enum Wrapper<T: Tagged = Int> { Item { value: T } }` lowers today (`enum_generic_ir_lowering_tests.rs`) but nothing consumes the default — declared-but-inert (INV-SF-3). The structure analogue (`sub.type_args.get(position).or(tp.default.as_ref())`, `compile_builder/auto_type_param_phase.rs`) runs on `sub` **type positions**, i.e. annotations.
- **Counter-evidence against filling at construction (first draft):** a default turns "unknown" into a possibly-wrong concrete that then hard-conflicts — `Maybe.Nothing` filled as `Maybe<Int>` followed by `or_else(.., Just { value: 1mm })` is a spurious conflict where a placeholder would have unified.
- **Recommended:** annotations fill from defaults (this is also R2's exception); construction sites keep the R1 placeholder.

### R4 — Mint `E_ENUM_TYPE_ARG_UNRESOLVED`? — **RESOLVED (Leo, 2026-09-29, esc-6020-11 session)**: NO in this PRD; the first site that requires concreteness mints it in the same diff.

Under S1 no site must force an arg concrete: the placeholder is a legal static type, compared under the placeholder rule, never read by the evaluator. Minting now is PDEAD/PUNTESTED again (the superseded PRD §7.3, esc-4031-50). Recommended: the first site that REQUIRES concreteness — ζ's `f(E.V)` with no annotation, or the `/prd E` milestone's `None`-in-argument position — mints it in the same diff. Alternative: mint in ζ, fired only when a unit variant's args stay unbound after expected-type push-down.

### R5 — Match result type when arm bodies are placeholder-typed — **RESOLVED (Leo, 2026-09-29, esc-6020-11 session)**: the first arm whose body type carries no placeholder; if none, the placeholder. Coupled to R1's `is_unbound_placeholder` predicate.

- Today the match `result_type` is the first arm's body type. Fixtures `getar_match_err_first_arm_panics.ri` (Ok subject, `Err { error: e } => e` first) and `getar_match_err_subject_ok_first_panics.ri` (Err subject, `Ok { value: x } => x` first) still fail after δ alone: the first arm's binder is placeholder-typed, so the `let` cell is placeholder-typed and `is_representable_cell_type` rejects it.
- **(a, recommended)** first arm whose type carries no placeholder (needs R1's predicate); all-placeholder → placeholder (only reachable via a fully-unbound subject, which R2(b) removes). Closes P-1 for every arm order with no new diagnostic.
- **(b)** unify all arm types and diagnose disagreement — stronger, but arm-type agreement is not checked for non-generic matches today either; widens beyond D1.
- **(c)** annotate-or-error.

## §7 — Contract and boundary tests (B+H)

Seam: the shared overload tiers (`reify-core`, three consumers) and the type-compatibility core.

**Contract — static enum type (compile side):**
- C-1 A constructed generic variant's `result_type` is `Applied { name, args }`, `args.len() == type_params.len()`, each arg concrete when bound by payload or annotation, else the R1 placeholder. A non-generic variant's `result_type` is `Enum(name)`.
- C-2 Two enum types are compatible iff base names agree and, when both are `Applied`, arities agree and each arg pair is compatible: placeholder ≈ anything (including another placeholder, **never a conflict in `unify`**); concrete vs concrete by existing `type_compatible`. A bare `Enum(n)` value type is compatible with any `Applied{n,..}` (D-3).
- C-3 A type annotation pins args; a payload contradicting a pinned arg is `E_VARIANT_PAYLOAD_TYPE` (message "field … expects type …"), including through a nested generic field — the 4031 case is `E_ENUM_TYPE_ARG_CONFLICT` ("type parameter … bound to both …").
- C-4 Overload tier 3 admits a headed type-param-carrying arg only when heads unify, and a bare placeholder arg only against generic params; a bare user `TypeParam` arg keeps D4.
- C-5 Runtime `Value::Enum` is unchanged; no evaluator selects arms or values by static args. The one runtime reader of a declared type against an enum value, `value_type_kind_matches`, must admit `Applied` (θ) — a pre-existing gap, not S1-induced.

**Boundary tests (fixtures unless noted; POST rows need a rebuilt binary, `REIFY_BIN` set for the harness):**

| Row | Scenario | Pre | Post |
|---|---|---|---|
| B1 | `Ok { value: 5mm }` unannotated | δ | `result_type == Applied{Result,[Length, ?E]}`; `generic_enum_erasure_tests.rs` green (Rust) |
| B2 | S-1 `getar_fallback_type_mismatch_silent.ri` | γ, δ | check 1, stderr `E_FALLBACK_TYPE: ` |
| B3 | S-2 `getar_result_arg_mismatch_assign_silent.ri` | γ | check 1, stderr `argument 'b' has type` |
| B4 | S-3 `getar_tree_force_leaf_length_silent.ri` | δ | check 1, stderr `expects type` |
| B5 | 4031 `getar_recursive_conflict_silent.ri` | δ | check 1, stderr `bound to both` |
| B6 | U-1 `getar_fn_param_rejects_construct.ri` | β, δ | check 0; eval `W.v = 0.001 m` |
| B7 | P-1 `getar_match_over_let_construct_panics.ri`, `…annotated_let…` | δ | debug check 0, `W.v = 0.005 m` |
| B7b | R5 `getar_match_err_first_arm_panics.ri` (`W.v = "ok"`), `getar_match_err_subject_ok_first_panics.ri` (`W.v = 0.001 m`) | δ, ε | debug check 0 |
| B8 | S-4 `getar_wildcard_headed_arg_silent.ri`, `getar_wildcard_placeholder_binder_silent.ri` | β (second also δ) | check 1, stderr `no matching overload for fl(` / `for hb(` |
| B9 | non-generic `Circle { radius: 5mm }` | δ | `result_type == Enum("Shape")`, DCE goldens byte-identical (Rust) |
| B10 | R2(b) `getar_bare_result_annotation_match_panics.ri` | α2 | check 1, stderr `expects 2 type argument` |
| B11 | ε2 `getar_decl_match_applied_param_rejected.ri` / `getar_decl_match_let_construct_ok.ri` | ε2 | first: check 0; second: stays 0 after δ |
| B12 | θ: engine-admin override of `param r : Result<Length,String>` | θ | accepted (Rust e2e) |
| B13 | η: `parse_length_r("3mm")` assigned to `Result<Force,String>` | η, γ | check 1 |
| B14 | `or_else(Ok{value:1mm}, Ok{value:2mm})` (clean today) | γ, δ | still clean — the `unify` placeholder rule's regression guard |

## §8 — Cross-PRD / cross-task relationship (G4)

| Other PRD / task | Direction | Seam mechanism / shared files | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_6/generic-data-carrying-enums.md` (SHIPPED) | this supersedes the as-implemented erasure point of its D1 | `compile_variant_construct` result type; `base_enum_name` | this PRD | α edits its LIVE header only |
| **#7456** goal-oriented α, runtime `VariantCtor` (pending) | shared files, ordering-sensitive | `crates/reify-compiler/src/variant_construct.rs` (δ), `crates/reify-mcp/src/tools/chunks/enums.md` and spec §3.8/§4.5 (δ docs), `type_resolution.rs`; its WORK item 1 says "result type `Type::Enum(name)`" | **lead stamps #7456 now** with D-8: the node's `result_type` must be the `Applied` δ emits | queued |
| **#6020** Option match, route ii (deferred pending ruling) | shared files | `expr.rs` Match branch: binder resolver and exhaustiveness lookup, which #6020 extracts into ONE resolver `match_discriminant_enum` (with `option_enum_view`, type_params `[T]`); `enums.md`; spec §3.8 | #6020 owns the resolver; ε and ε2 route through it if it has landed (ε2's decl-form check should consume the same resolver rather than re-implement the `Applied` case) | queued |
| **#5689** overload-tier hoist (done) | substrate | `slot_matches_*_tier` in `reify-core`, three consumers | landed | wired (β edits one predicate) |
| **#5992** compiler/eval overload parity test (pending) | independent | policy layers above the shared tiers | — | no edge |
| `docs/prds/v0_6/result-and-fallback.md` Layer B (SHIPPED) | this produces, that consumes | `E_FALLBACK_TYPE` on unannotated subjects | this PRD (γ+δ) | B2 |
| **#4705** let annotation-vs-initializer check (done, scalar/collection halves) | independent | `check_let_annotation_type` early return for enum-typed lets | follow-up filed at decompose | §10 |
| `/prd E` milestone (not yet authored) | this produces, that consumes | `Applied{Option,[T]}` distinguishability | this PRD | milestone task depends on β, γ, δ |
| #6017 / #7751 (match variant validation) | independent | none | — | n/a |

## §9 — Decomposition plan (B+H; filed 2026-09-29 as #8012 α · #8013 α2 · #8014 β · #8015 γ · #8016 ε2 · #8017 θ · #8018 δ · #8019 ε · #8020 ζ · #8021 η · #8022 ι · #8023 κ)

Rule: one orchestrator leaf = one mergeable TDD diff confined to one layer. Probe sets under `tests/prd-gate/` are split **per leaf** so a leaf's dispatch-time check never depends on a downstream leaf's rows; α lands them with their README rows (see α's row for why they are not in the docs commit).

| Leaf | Title | Modules | Observable signal | Deps |
|---|---|---|---|---|
| **α** #8012 | Docs root: `generic-data-carrying-enums.md` LIVE header points here; land the six per-leaf probe sets `tests/prd-gate/generic-enum-type-arg-retention-{alpha2,beta,gamma,epsilon2,delta,epsilon}-probe-set.json` (contents embedded verbatim in the task) and their rows in `tests/prd-gate/README.md` "Committed probe sets" — the `.json` files could not ride the direct docs commit: `decide_scope` classifies `tests/prd-gate/*.json` as the C5 fail-wide catch-all (`RUN_RUST=1`, all crates; pinned by `test_verify_scope.sh` PG-3), so they land through the merge queue with α | `docs/prds/`, `tests/prd-gate/` | the LIVE pointer present; `python3 scripts/prd-capability-check.py` runs each set (red rows expected) with rc ≠ 64; README rows present | — |
| **α2** #8013 | Bare generic-enum annotation rejected with the existing arity diagnostic unless declared defaults fill every parameter (R2(b)+R3) | reify-compiler `type_resolution.rs` (`resolve_enum_type_with_args` zero-arg path) | `getar_bare_result_annotation_match_panics.ri` check 1, stderr "expects 2 type argument" (B10); probe set `…-alpha2-probe-set.json` | α |
| **β** #8014 | Narrow tier 3: headed type-param-carrying args need `heads_unifiable`; bare placeholder args match generic params only; bare user `TypeParam` keeps D4 | reify-core `overload.rs` (+ R1 predicate if it lives in core) | `getar_wildcard_headed_arg_silent.ri` check 1 "no matching overload for fl(" (B8); probe set `generic-enum-type-arg-retention-beta-probe-set.json` | α |
| **γ** #8015 | Args-aware enum-compat oracle; `unify` placeholder no-conflict rule; conformance + `implicitly_converts_to` placeholder rule; corpus sweep | reify-compiler `type_compat.rs`, `conformance/mod.rs` | `getar_result_arg_mismatch_assign_silent.ri` check 1 "argument 'b' has type" (B3); B14 stays clean; probe set `…-gamma-probe-set.json`; `examples_smoke` green | α |
| **ε2** #8016 | `MatchArmDeclGroup` discriminant accepts `Applied` (via #6020's resolver if landed) | reify-compiler `entity.rs` | `getar_decl_match_applied_param_rejected.ri` check 0 (B11); `getar_decl_match_let_construct_ok.ri` stays 0; probe set `…-epsilon2-probe-set.json` | α |
| **θ** #8017 | `value_type_kind_matches` admits `Applied` for `Value::Enum` | reify-eval `lib.rs` | engine-admin override of an annotated `Result` param accepted (B12) — test added to the standalone binary `crates/reify-eval/tests/generic_enum_erasure_e2e.rs` (baselined in `tests/infra/harness-layout-baseline.manifest`) | α |
| **δ** #8018 | `compile_variant_construct` emits `Applied` (R1 placeholder for unbound); 4031 test; flip the two `result_prelude_enum_tests.rs` pins; DCE goldens byte-identical; **docs-truth same diff:** spec §3.9.2 static-type sentence, `enums.md` construction-inference sentence | reify-compiler `variant_construct.rs`; `docs/reify-language-spec.md`; `crates/reify-mcp/src/tools/chunks/enums.md` | B2, B4, B5, B6, B7 (fixture-backed); probe set `…-delta-probe-set.json`; `harness_doc_chunks` scrape green | β, γ, ε2 |
| **ε** #8019 | Match result-type rule (R5) | reify-compiler `expr.rs` Match branch (through #6020's resolver if landed) | B7b: both R5 fixtures check 0 under `REIFY_BIN=target/debug/reify` (probe set `…-epsilon-probe-set.json`, debug binary required); Rust debug-profile test asserting the let cell types; fixtures registered in `_RUST_COUPLED_RI_FIXTURES` | δ |
| **ζ** #8020 (optional under R4, RULED: threading only, no diagnostic) | EnumAccess unit-variant expected-type threading | reify-compiler `expr.rs` EnumAccess arm | a generic unit variant assigned to an annotated param has `Applied` static type (leaf declares its fixture; no generic unit variant is compiled in tree today) | δ |
| **η** #8021 | `parse_length_r` registered as `Result<Length, String>`; pins in `registry_seed_result_types.rs`, `parse_length_signatures.rs`, `registry.rs` in-crate test updated | reify-builtins `registry.rs` + tests | B13 check 1 | **θ**, γ |
| **ι** #8022 | Accept-side exemplar `examples/best_practices/generic_enum_typing.ri` (B6/B7-shaped constraints only; rejections stay in fixtures) + `INDEX.md` line + reify-design cheatsheet index line + CLI e2e | examples, CLI harness | `reify eval` on the example exits 0, every constraint Satisfied; `examples_smoke` green; discoverability line present | δ, ε, η |
| **κ** #8023 | PRD-close: terminal `SHIPPED` stamp, landed leaf IDs, AS-AUTHORED freeze + LIVE map on PRD and manifest | docs | the committed header | every other leaf |

Docs-truth gate (overlay): language surface changes (new diagnostics on previously-accepted code; U-1 legal). The four arms: δ (spec + chunk, same diff — the sentence "a constructed variant's static type is the applied type" is false until δ lands and `enums.md` is under `crates/`, so neither can be an α docs commit), ι (exemplar corpus + `INDEX.md` + cheatsheet index + intent-level discoverability: "why does my Result parameter accept the wrong unit?" finds the enums chunk).

Drift-guard registration: ε/θ/ι add tests inside existing harness roots or the existing standalone binary; no new gate-resident binary, no wall-clock bound. Any fixture read via `include_str!` is added to `_RUST_COUPLED_RI_FIXTURES` in the same diff (PG-DRIFT).

## §10 — Out of scope

- **S2 runtime retention.** No consumer; hash/Eq/Ord churn.
- **Bidirectional / expected-type inference for fn arguments.** `resolve_function_overload` is bottom-up; `unwrap_or(Err { error: "x" }, 0mm)` keeps a placeholder `T`. Its own PRD; the `/prd E` milestone's third named precondition.
- **The let form of S-2** — `getar_let_annotated_result_arg_mismatch_silent.ri` stays silent after γ because `check_let_annotation_type` returns early for non-scalar declared types (#4705 landed the scalar/collection halves). Decompose files the enum-annotation follow-up against #4705 naming that fixture.
- **Option migration.** `Type::Option` / `Value::Option` untouched (D-7).
- **Nested patterns** (DCE §10).
- **Structure constructor result typing** (`Foo(...)` → `StructureRef`).
- **`auto` type params on enums** — still unexercised (superseded PRD §11 Q2).

## §11 — Open questions (tactical; decide at impl)

1. **Placeholder `Display` spelling** (`?E` vs `Result<Length, _>`) in diagnostics and hover — it is `Type`'s `Display` (the GUI hashes `cell_type.to_string()`; `format_hover` formats values, not types). Decide at δ; **must be site-independent** (R1).
2. **Diagnostic code for S-2** — reuse ctor-conformance `ArgTypeMismatch` vs a dedicated code. Decide at γ; prefer reuse.
3. **Corpus sweep scope for γ** — `examples/`, stdlib, `prj/`, `designs/` (the last two are outside `examples_smoke`).
4. **Placeholder in `Type` `Hash`/`Eq`/`Display`** — same name at every site (R1c), so two placeholders for the same param are `Eq`; `Display` carries no counter. Confirm no existing `Type` consumer keys on `TypeParam` names being user-visible.
5. **Where the `is_unbound_placeholder` predicate lives** — `reify-core` (next to `type_carries_type_param`, so β can use it) is the natural home.

## Gate walk (author mode, 2026-09-29, post-critic)

- **G1 PASS** — six consumer groups in §1, each observable or a filed task; #5992 demoted from consumer to non-edge.
- **G2** — decompose-time; every leaf row is fixture- or Rust-test-backed; β now has its own signal (S-4) and is no longer an intermediate.
- **G3 PASS** — no novel grammar (all 15 fixtures: 0 Lezer error nodes; all reach the type checker); substrate: `Type::Applied`, `unify`'s Applied arm, `final_subst`, the `__auto_` reserved-prefix precedent, all present on main `300910857b`.
- **G4 PASS** — §8 names #7456 and #6020 with their shared files and the resolver they must share; the lead stamps #7456 before decompose.
- **G5 B+H** — §7.
- **G6 PASS** — every signal is a measured before-image on both binaries with exit code, value and the diagnostic substring the mechanism emits; no numeric bounds.
- **G7 advisory** — INV-SF-2/3/5/6 walked; no waiver.
- **META PASS** — R1–R5 ruled and recorded (§6, D-10); no design question remains open. Decomposed 2026-09-29.
