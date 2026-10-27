# PRD: Generic instantiation and call binding — bind once, record it, keep the eval graph concrete

**Status:** active — authored 2026-10-17/18 by the gap-fill stream session for
**generic-instantiation-and-call-binding** (`plans/gap-fill-deliberation-reify-20261016.md` §K1-D1, §K1-D2,
the uncontested block's K1-D3 bullet with C14, §G1). Decomposed in the same session (2026-10-18): α₁ #9358,
α₂ #9359, β₁ #9360, β₂ #9361, γ₁ #9362, γ₂ #9363, δ #9364, ε #9365, ω #9366.
Shape: **B+H** (contract §6, boundary-test sketch §7). Milestone: v0_6. Size L, risk high, program wave 3.

**Code anchors** verified against main `baff3ed38b` (2026-10-17). `git diff --stat baff3ed38b..5d07b395db`
over this stream's source paths touches `orientation_signatures.rs`, `unresolved_function.rs`, 41 lines of
`crates/reify-compiler/src/expr.rs` (#6306), comment re-cites in `crates/reify-core/src/diagnostics.rs`,
`reify-eval`'s `engine_build.rs` / `compute_targets/elastic_static.rs`, 21 lines of
`crates/reify-lsp/src/completion.rs`, and test files; none of the symbols below changed. Main moves fast:
cite by symbol, and re-locate lines at implementation time. Probe outputs below are from `target/debug/reify`
built 2026-10-16 14:45; no compiler/eval/ir/expr/core source change except a comment separates it from
`baff3ed38b`.

**Finding keys owned (6):** fk-4bd738a79576 (high), fk-7bf9e6299eca (high), fk-1b3f91469997 (high),
fk-7eb49b30a889 (medium), plus fk-0e4c27b21db7 (medium, K1-D3's interim #9329, absorbed) and fk-6b74a4f8fcfc
(low, interim #9169, which lands first). All six are hotspot-survey-reify-20261015 keys; statements and
proposals are in `docs/notes/bug-hotspot-survey-2026-10-15-full-findings.json`. §10 maps each key to its leaf.

---

## §1 Goal

The compiler decides three things about generic code, and today it decides each of them more than once, in
places that disagree:

1. **Type-argument binding.** Given a constructor's declared type parameters (with optional defaults) and the
   arguments written at a use site, is the arity legal, and what is the binding Σ? Eleven code sites decide
   this, under four arity policies and seven message shapes, and several are silent.
2. **Instantiation.** Turning a generic `TopologyTemplate` into a concrete one is done only inside the
   `auto:` resolver, by cloning and substituting a hand-picked subset of fields. An explicit
   `Holder<GasketSeal>()` is never instantiated at all; two monomorphs share cell ids.
3. **Call binding.** The compiler picks a user-function overload and its type arguments, then throws the
   answer away: `UserFunctionCall` carries only `function_name`. The compiler's own post-pass and eval each
   re-derive the choice under their own policies.

After this PRD the compiler computes Σ and the overload choice **exactly once**, records both on the IR, and
enforces "every template reaching the eval graph is concrete" at construction and at hydration. Eval, the LSP
and the compiler's post-pass stop re-deriving or compensating.

A user observes:

- `param h : Holder<Bolt>` compiles when `Holder<U, T = Steel>` declares a default, exactly as
  `sub part = Holder<Bolt>()` already does, and the two spell the same type.
- `sub h = Holder<GasketSeal>()` evaluates (`Top.h.seal = GasketSeal { thickness: 2 }`) instead of panicking
  `reify check` in debug and degrading to `Undef` in release. A declared-but-never-instantiated generic no
  longer panics either.
- Two `Widget<…>` monomorphs keep separate cells; `reify eval` no longer prints a last-write-wins
  `Widget.slot_t`.
- An applied generic enum resolves at any depth and through an alias body (`Option<Ge<Real>>`, `type W =
  Ge<Real>`), as the top-level `param p : Ge<Real>` already does.
- Every arity mistake gets one coded diagnostic naming the constructor, the accepted range and the actual
  count: `Int<Real>` and `Orientation<2>` are rejected instead of silently discarded, and an arity mismatch
  inside an alias body names the inner alias instead of printing "unresolved type".
- A trait instance-method call's arguments are checked against the chosen callee exactly as a free
  function call's are.
- A parametric alias body resolves exactly as its direct spelling does.

Under this PRD a new generic position (generic traits, #5024) is one `TypeArity` arm, a new
`TopologyTemplate` field is a compile error in `instantiate_template` until someone decides how it
substitutes, and a new overload rule is one edit in `bind_user_call`.

## §2 Background: what the code does at `baff3ed38b`

### §2.1 Type-argument binding has no home (fk-4bd738a79576, fk-0e4c27b21db7, fk-6b74a4f8fcfc)

The arity sites (files under `crates/reify-compiler/src/` unless stated; "range" = `[required, total]`,
where required counts the parameters before the first default):

| # | site | policy | defaults | diagnostic |
|---|---|---|---|---|
| A1 | `type_resolution.rs::resolve_parameterized_alias` | range | padded | code-less `type alias 'X' expects R-N type argument(s), got G` |
| A2 | nested-alias arm of `type_resolution.rs::resolve_type_alias_expr_with_subst` | range | padded | **silent `None`** (#9169 makes it loud) |
| A3 | `type_resolution.rs::resolve_enum_type_with_args` | strict | no; a bare generic enum erases to `Type::Enum` | code-less `` enum `E` expects N type argument(s), found G `` |
| A3b | `conformance/checker.rs` enum fallback | arity 0 always | no | code-less `does not accept type arguments` |
| A4 | `type_resolution.rs::check_applied_type_arg_bounds` | strict | **ignored** | `TypeArgArity` (its only emitter) |
| A5 | projection arm of `type_resolution.rs::normalize_type_guarded` | strict | no | `UnresolvedType` `type argument count mismatch …` |
| A6 | `entity.rs::check_type_param_bounds` (+ `effective_arg`) | range | yes | code-less `too many …` / `missing type argument …` |
| A7 | `compile_builder/auto_type_param_phase.rs` seeding (`effective_arg` copy, "mirrors" comment) | explicit else default | yes | silent skip |
| A8 | `type_resolution.rs::resolve_entity_name_with_shadowing`: step 1 builds `Type::Applied` from the written args unpadded and unchecked; step 3 (bare-name fallthrough) **drops** args on builtin, type-param, non-parametric-alias and dimension names | none | no | silent |
| A9 | `variant_construct.rs` pin guard | strict | no | none (falls back to inference) |
| A10 | `expr.rs` match `binder_subst` | unchecked zip | no | none |

Plus: 24 per-builtin `type_args.len() == N` guards (12 in each of the direct and `_with_subst` resolvers)
falling to a silent `_ => None`; the def-site prefix zip in `validate_pub_parametric_alias_def_site`; the
`entity.rs` Named-flatten sites that resolve nested type args with `resolve_type_name` (#8229); and the bare
enum payload arm in `compile_builder/enums_phase.rs` (#8098). `reify_ir::TypeParam { name, bounds,
default: Option<Type> }` lives in `crates/reify-ir/src/traits.rs`; Σ is a bare `HashMap<String, Type>`
everywhere; `DiagnosticCode::TypeArgArity` is defined in `crates/reify-core/src/diagnostics.rs`.

Probes (fixtures committed with this PRD under `tests/prd-gate/fixtures/`):

- `gicb_alpha_default_padding.ri` (probe a): `sub part = Holder<Bolt>()` is accepted;
  `param h : Holder<Bolt>` fails `error: wrong number of type arguments for 'Holder': expected 2, got 1`, rc 1.
- `gicb_alpha_nested_alias_arity.ri` (probe c): `type P<T> = List<T>; type Q<X> = P<X, X>` prints only
  `error: unresolved type: Q<Int>`; the direct `P<Int, Int>` gets `type alias 'P' expects 1 type
  argument(s), got 2`. (The survey's spelling with a `= [1]` default adds a `list literal cannot satisfy
  annotation` cascade; the fixture omits the default so the signal counts one error.)
- `gicb_alpha_nongeneric_builtin_args.ri` (probe e, #7337): `Int<Real>`, `Length<Int>`, `Bool<Length>`,
  `String<Int>` and `Orientation<2>` all check clean, rc 0.
- `gicb_alpha2_sub_arity_excess.ri`: `Holder<Bolt, Steel, Steel>()` prints the code-less
  `too many type arguments for 'Holder': expected 2, got 3`.
- `gicb_alpha_nested_generic_enum.ri` (#8214): `Option<Ge<Real>>`, `List<Ge<Real>>` and `type W = Ge<Real>`
  each fail `unresolved type`, rc 1. Enum arity exists only as the caller-level fallback
  `resolve_enum_type_with_args` (called from five declared-type positions once the shared resolver returns
  `None`); the shared resolver sees enums through a names-only ambient set, gated on `type_args.is_empty()`.
  Only a top-level `param p : Ge<Real>` resolves.

`Orientation<N>` / `Frame<N>` / `Transform3<N>`: `Type::Orientation(usize)`, `Frame(usize)` and
`Transform(usize)` carry an `n`, but only N=3 is inhabited (the value is a unit quaternion;
`try_infer_type` yields `Orientation(3)` unconditionally; `ty.rs` calls the parameter vestigial). The bare
names resolve to N=3, and a written `<N>` is discarded, so `Orientation<2>` silently means the 3D type.
Whether SO(2) becomes a type is #6336's decision (gated on the v1 sketch family), and #6336 records that it
should be a distinct type, not this `usize` at 2. 48 doc lines spell the family with an argument
(`git grep -E '(Orientation|Frame|Transform3?)<[0-9N]>'` over `docs/reify-stdlib-reference.md`,
`docs/reify-language-spec.md` and the MCP chunks, 2026-10-18); no `.ri` code does.

### §2.2 Instantiation is owned by the `auto:` resolver (fk-7bf9e6299eca)

`compile_builder/auto_type_param_phase.rs::phase_auto_type_param_resolution` returns immediately when no
`auto:` use site exists. Otherwise it builds each monomorph with `mono = target.clone(); mono.name =
mono_name; mono.type_params.clear()` (name from `types.rs::mangle_monomorph_name`, `Generic$A$B`) and
substitutes field by field through `substitute_value_cell_collection`, `substitute_constraint_collection` and
inline loops. `TopologyTemplate` lives in `crates/reify-compiler/src/types.rs` (not reify-ir). Coverage:

- substituted: `value_cells`, `constraints` (result types only), `guarded_groups`, `ports`,
  `sub_components[].type_args`;
- not substituted: `realizations`, `relations`, `connections`, `objective`, `match_arm_groups`,
  `forall_templates`, `assoc_fns`, `assoc_types`, `structure_controlling`, and `sub_components[].args`,
  `pose`, `auto_pose`, `keyed_member_overrides`, `count_cell`.

There is no exhaustive destructure, so a new field compiles unsubstituted. The postcondition `debug_assert!`
checks top-level `value_cells` for a direct `Type::TypeParam` only.

**No `ValueCellId` is re-keyed.** `ValueCellId { entity: String, member: String }` lives in
`crates/reify-core/src/identity.rs`; a monomorph's cells and every `ValueRef` in it keep the generic's
template name. Template-level cells with path-rooted entities exist too (`entity.rs` builds
`ValueCellId::new(format!("{}.{}", entity_name, sub.name), member)` for override and auto cells).

**Explicit instantiation is never monomorphised.** An `AutoResolutionRequest` is pushed only when a sub has
`auto:` clauses; `sub h = Holder<GasketSeal>()` keeps pointing at the generic, and reify-eval substitutes
nothing.

**Hydration admits generic templates.** `crates/reify-eval/src/graph.rs::EvaluationGraph::from_templates`
hydrates every template. The only backstop is `#[cfg(debug_assertions)]
assert_value_cell_types_representable` (`engine_eval.rs`, called from `Engine::eval` and `edit_source`). Its
panic message and four other cites name `value_cell_type_invariants.rs` at the reify-eval tests root, but the
test lives at `crates/reify-eval/tests/harness_tolerance/value_cell_type_invariants.rs` (five stale cites). `first_unrepresentable_cell` exists only as a private function in
`crates/reify-lsp/src/diagnostics/eval_guard.rs`; S27 β #8761 moves it into reify-eval.

Probes:

- `tests/prd-gate/fixtures/tooling_fe_unrepresentable_generic.ri` (declared `Bearing<T: Seal>`, never
  instantiated): `reify check` **and** `reify eval` panic in debug, rc 101, `value cell 'Bearing.seal' has
  cell_type TypeParam("T")`.
- `gicb_beta_explicit_instantiation.ri` (#6853): `reify check` panics, rc 101, on `Holder.seal`; `reify eval`
  panics on `Top.h.seal`.
- `gicb_beta_two_monomorph_collision.ri` (probe k): `reify eval` prints `Top.w1.slot_t = SealA { a: 1 }`,
  `Top.w2.slot_t = SealB { b: 2 }` **and** `Widget.slot_t = SealB { b: 2 }`: the generic's own cell is
  written by both monomorphs, last write wins.

### §2.3 Call binding is thrown away (fk-1b3f91469997)

`CompiledExprKind::UserFunctionCall { function_name: String, args }` (`crates/reify-ir/src/expr.rs`). The
compiler lowers it at five sites inside `expr.rs::compile_expr_guarded_with_expected_inner`: the `Resolved`
arm, the default-padding branch, the selector-coerced branch (each going through
`type_compat.rs::resolve_function_overload` or its padding/coercion helpers, then
`expr.rs::instantiate_call_return_type`), the `TraitMethodCall` instance-assoc-fn arm (its own ladder:
`slot_matches_wildcard_tier` + an exact tiebreak, ending `resolved[0].return_type.clone()`), and
`TraitStaticCall`. Two consumers rebuild the answer:

- `compile_builder/entities_phase.rs::check_expr_fn_calls` re-runs `resolve_function_overload` and re-unifies
  params against args. Its comment "same logic as expr.rs:1530" is stale (`instantiate_call_return_type`).
- `reify-expr/src/lib.rs::find_matching_compiled_function` (an ~86-line doc on keeping agreement with the
  compiler) re-selects the overload at eval, first-match-wins over three tiers, from ten production callers
  (`eval_user_function_call`, `dual_eval.rs`, five in `engine_eval.rs`, two in `invariants.rs`,
  `unfold/optimized_instance_reuse.rs`). `None` becomes a cause-less `Undef`.

The two tables also differ: the compile table is `merge_prelude_functions` (shadowed prelude filtered), the
eval table `reify-eval/src/lib.rs::merge_functions` (user first, prelude appended unfiltered). Drift has
shipped three times (esc-4093-152, #5685, #5686); #8586 says a widening rule must be implemented "in BOTH".
The `content_hash` of a call is `TAG ⊕ name ⊕ args`, folded in three places (reify-ir, the compiler's
`build_user_function_call_expr`, the reify-test-support builder).

Probe (measured this session; the brief's claim that padded calls skip the post-pass is false — a padded node
carries padded args and re-resolves):

- `gicb_gamma_trait_method_arg_conformance.ri`: `sc.(Weigher::weigh)(Foam())` with `other: Rigid` checks clean,
  rc 0, and `reify eval` prints `Top.bad = 1`. `phase_register_instance_assoc_fns` runs after
  `phase_fn_arg_conformance`, so at check time the callee is in no table and both checks are skipped. The
  free-function spelling (`weigh(Foam())`) errors `type 'Foam' does not conform to trait 'Rigid' required by
  param 'other'`.

### §2.4 Alias bodies are resolved by a second resolver (fk-7eb49b30a889)

Two TypeExpr resolvers: `type_resolution.rs::resolve_type_expr_with_aliases_kinded` (direct spellings) and
`resolve_type_alias_expr_with_subst` (alias bodies, with its own Arrow, nested-parametric-alias, `_with_subst`
builtin twin, `_to_dim_with_subst` twin and depth-guard arms). They differ on one axis: whether a parameter
name resolves to a declared `TypeParam` leaf or is looked up in a bound Σ. `resolve_entity_name_with_shadowing`
is shared, and its doc lists the arms the alias-body path lacks (the deferred non-parametric alias arm, a real
gap; the `E_BARE_SCALAR` guard). `mod parametric_alias_deferred_alias_body_known_gap`
(`crates/reify-compiler/tests/harness_langcore/type_alias_compile_tests.rs`) asserts `alias == direct || alias
reports an error` over four rows, so it is green today and stays green whether or not the gap closes.

Probes: `gicb_delta_alias_body_deferred_entity.ri` (`type Outer<T> = Option<Inner>` with `type Inner = Zq`)
fails `error: unresolved type: Outer<Real>`, rc 1, while the direct `Option<Inner>` resolves;
`gicb_delta_alias_body_generic_enum.ri` (`type W<U> = Ge<U>`) fails `error: unresolved type: W<Real>`.

## §3 Pre-conditions and already-filed owners

Every prerequisite is a filed task; the "held" leaves of the ruling are plain dependency edges (R3).

| prerequisite | owner | what this stream consumes | edge |
|---|---|---|---|
| `ResolutionCtx` (immutable value; `with_*` constructors; kinded/non-kinded entry collapsed; three thread-locals replaced) | S7 μ #8643 | α₁ adds a constructor (structure + enum) type-param table to it; δ adds `TypeParamEnv` | α₁, δ ← #8643 |
| D13 `ParamBuiltin { name, arity, slots, build }` table; `resolve_parameterized_builtin_type(name, args, ctx, subst)` | S8 ο #8903 | α₁ reads `arity` (one source); δ deletes the `subst` parameter | α₁, δ ← #8903 |
| `Type::children` / `Type::any_descendant` | S7 λ #8638 | `Sigma::is_concrete`, `template_is_concrete` | α₁, β₁ ← #8638 |
| `TopologyTemplate::all_value_cells()` (ratchet R4) | S9 ν #8743 | β₁'s destructure walks cells through it | β₁ ← #8743 |
| `InstancePath` (parse/Display byte-identical; `.entity` parsing ratchet at 0) | S3 ι #8704 | β₁'s re-key (R7) | β₁ ← #8704 |
| UserFunctionCall `intrinsic: Option<StdlibIntrinsic>` stamp; `classify_user_call` | S8 ζ #8897 | γ₁ replaces the field by a projection of `callee.origin` | γ₁ ← #8897 |
| `first_unrepresentable_cell` in reify-eval (eval-readiness predicate) | S27 β #8761 | β₂ rewrites it over `Type::has_value_representation` or deletes it (one representability definition) | β₂ ← #8761 (follow-on, G1) |
| engine_eval.rs partition (`cell_type_invariants.rs`) | S4 ν #8860 | β₂ edits the moved assert's home | β₂ ← #8860 |
| interim nested-alias arity diagnostic (`alias_arity_accepts`) | #9169 (branch `task/9169`, 2 commits) | α₁ deletes the helper and keeps its test | α₁ ← #9169 |
| compile/eval overload parity test | #5992 (branch `task/5992`, 14 commits) | γ₂ deletes it with the second resolver (R1) | γ₂ ← #5992 |
| zero-argument generic-enum arity diagnostic in `resolve_enum_type_with_args` (spec §3.9.2: a bare generic enum annotation is an arity error unless every parameter is defaulted) | generic-enum-type-arg-retention α2 #8013 (branch `task/8013`, 5 commits, 2026-10-18) | α₁ routes A3, including #8013's bare-annotation helper and #8098's sites, through `bind_type_args` | α₁ ← #8013 |
| generic-enum `Applied` exits of `variant_construct.rs::compile_variant_construct` | generic-enum-type-arg-retention δ #8018 | α₂ replaces the pin guard (A9) after δ's exits land | α₂ ← #8018 |

#8761, #8860 and #8704 are in #8612's dependency closure (recomputed live 2026-10-18: 246 tasks including the
root). Depending **on** them is allowed; no leaf of this stream is a prerequisite of any closure task (G1).
The front end (S27 γ #8778, resolution-unification β #5516 `compile_program`) is consumed unchanged.

## §4 Rulings

### From the record (binding; full text in `plans/gap-fill-deliberation-reify-20261016.md`)

- **K1-D1** (human, 2026-10-16): A. Adopt a new spawn stream `generic-instantiation-and-call-binding`
  (L/high, wave 3).
  - α `type_args.rs`: `TypeArity` from every constructor kind; `bind_type_args(arity, args, ctor, span) ->
    Result<Sigma, Diagnostic>`; one TypeArgArity diagnostic; `Sigma` complete by construction; every arity
    site calls it; bare-name fallthrough is arity 0.
  - β `compile_builder/monomorph.rs::instantiate_template(&TopologyTemplate, Sigma)` with an exhaustive
    destructure, ValueCellId re-keyed under the monomorph name and a `template_is_concrete` postcondition;
    called by the auto phase and a new explicit-instantiation pass; reify-eval graph hydration skips generic
    templates and checks the same predicate always-on (never changing the CLI exit policy, S27 OQ3-A).
  - γ `CompiledExprKind::UserFunctionCall { callee: FnRef, type_args }` produced once by `bind_user_call` at
    the five lowering sites; `check_expr_fn_calls` reads it; reify-expr dispatches on `callee` and
    `find_matching_compiled_function` is deleted; S8 ζ's `intrinsic` stamp becomes a projection of callee
    origin; `content_hash` folds callee.
  - Held leaves: α/δ behind S7 μ #8643 and S8 ο #8903; γ behind S8 ζ #8897.
  - Applied: the stream ports and cancels #7337, #8229, #8098, #9169, #7573, #6853, #7225, #6851 and (if no
    branch is in flight) #5992 when its leaves are filed; edge #8586 ← γ.
- **K1-D2** (human, 2026-10-16): A. Leaf δ, after S7 μ #8643 and S8 ο #8903: `ResolutionCtx` gains
  `TypeParamEnv { Declared | Bound(&Sigma) }`, derived by value; `resolve_type_alias_expr_with_subst` and
  D13's `subst` parameter are deleted; the `parametric_alias_deferred_alias_body_known_gap` pin turns green
  and is retired; #7573 is absorbed.
- **K1-D3 + C14** (uncontested block, 2026-10-16): a medium mechanical task (filed as #9329) pads Applied args
  with declared defaults at construction and range-checks `[required, total]` in
  `check_applied_type_arg_bounds`, with the probe-(a) pair as a compile test; subsumed by α if α lands first.
  **C14:** #6851 stays as filed as the interim fix-site-1 for the shape-2 panic; K1 β subsumes the remainder.
- **G1** (human, 2026-10-16): #8612's transitive dependency closure is protected; no gap-fill leaf becomes a
  prerequisite of a closure leaf; a substantive widening of a closure leaf takes follow-on form.

### This session (Leo, 2026-10-17/18; answers verbatim)

- **R1 — #5992** (14 unmerged commits). "Let it land; γ deletes it (Recommended)". #5992 stays pending and
  lands; γ₂ deletes its parity test and the `__resolve_function_overload_for_parity_test` shim with
  `find_matching_compiled_function`.
- **R2 — #6851.** "C14 governs (Recommended)". Not cancelled; β₁ ports its fixtures, entry-point note and
  shape-2 counterexample and does not depend on it.
- **R3 — held leaves.** "Plain dependency edges (Recommended)". Every leaf is filed pending with
  `add_dependency` onto the filed prerequisites; no `x_seam_wait`.
- **R4 — follow-on cleanup.** "Stream owns a follow-on leaf (Recommended)". β₂, after β₁ and #8761.
- **R5 — FnRef.** "Signature key (Recommended)". `FnRef { name, params: Vec<Type>, origin: Local|Prelude }`;
  ζ's `intrinsic` field becomes `intrinsic(&callee)`; one `FnTable::merge` for compile and eval.
- **R6 — content_hash.** "Always fold FnRef (Recommended)". `TAG ⊕ callee.content_hash() ⊕ args`;
  `type_args` not folded; every user-call hash moves once.
- **R7 — re-key.** "Through S3 ι's InstancePath (Recommended)". β₁ depends on #8704.
- **R8 — instantiation pass.** "One instantiation pass (Recommended)". The auto phase only binds `auto:`
  slots; `phase_instantiate_generics` instantiates every sub with a concrete Σ. The dated addendum to
  `docs/prds/v0_3/auto-type-param-resolution-completion.md` §4.2 invariant 2 / §4.3 lands in β₁'s diff.
- **R9 — `Orientation<N>` / `Frame<N>` / `Transform3<N>`.** Leo first asked: "what do these type parameters
  actually model? Is Orientation<3> a 3D orientation? So Orientation<2>`would be 2D orientation? Is that
  useful in sketches? Same for Frame<N>? What does the N mean in Transform3<N>? What use cases do we have, or
  can we think of, for values of N other than whatever it defaults to when those args are silently discarded?
  What does it default to?" Answered from the code (§2.1; #6336), then ruled: "Arity 0; respell the docs
  (Recommended)". A written argument on these names is an arity error until #6336 rules; ε respells the docs.
- **R10 — hydration channel.** "Error diagnostic, eval continues (Recommended)". One
  `E_UNREPRESENTABLE_CELL_TYPE` Error in `EvalResult.diagnostics`; the CLI policy is unchanged.
- **R11 — deletion-leaf signals.** "Accept, stated in the PRD (Recommended)". §9 states γ₂'s and β₂'s
  signals as deletion-leaf signals.
- **R12 — conventions.** "Accept all three (Recommended)": (1) `type_args: Vec<Type>` in declaration order,
  unbound = `Type::unbound_placeholder(name)`; (2) a padded `Applied` prints padded; (3) `__auto_*` and
  `__unbound_*` placeholders are non-concrete. (3) is corrected by R13.
- **R13 — what "concrete" means** (asked after the critic found that a deep "no `TypeParam` anywhere"
  predicate rejects valid templates: generic-enum R1 gives `let r = Ok(1)` the cell type
  `Applied{Result, [Int, TypeParam("__unbound_E")]}`, and a generic fn returning `Option<T>` with `T` unbound
  nests `TypeParam("T")` into a let cell). "Representable + own params gone (Recommended)".
  `template_is_concrete` = no type params and every hydrated cell type has a value representation (eval's
  existing shallow predicate, one home), checked at construction and hydration; `instantiate_template` adds a
  deep postcondition that no `TypeParam` names one of the generic's own parameters; a placeholder in a Σ
  argument keeps that sub uninstantiated, but placeholders stay legal in derived positions of an instantiated
  template.
- **R14 — #8214** (asked after the decompose-verify workflow found that δ's `type W<U> = Ge<U>` deliverable has
  no applied-enum arm to resolve through). "Absorb #8214 into α₁ (Recommended)". α₁'s constructor table
  covers structures and enums; the shared resolver gains the applied-enum arm through `bind_type_args`; the
  caller-level `resolve_enum_type_with_args` fallbacks retire onto the shared path; #8214 is ported and
  cancelled with `x_subsumed_by` α₁.

## §5 Resolved design decisions

- **D1 — complete, concrete, representable** (R13). Two landed rulings keep deliberately unbound arguments
  as types: generic-user-functions D2/INV-3 (unmentioned parameters stay unbound) and generic-enum R1
  (`Type::unbound_placeholder`, `__unbound_<P>`). Three predicates, each with one home:
  - a `Sigma` is **complete** by construction: one `Type` per declared position, the written argument else
    the default;
  - `Sigma::is_concrete` is deep: no `TypeParam`/`ScalarParam` descendant in any argument. A sub whose Σ is not
    concrete (an enclosing parameter, a placeholder, a failed `auto:`) is not instantiated now;
  - `template_is_concrete` is what eval needs: no declared type params, and every hydrated cell type passes
    the existing shallow value-representation predicate (`is_representable_cell_type`, moved beside `Type`).
    Placeholders and a generic fn's unbound parameters stay legal in derived positions (let cell types, arm
    types); `instantiate_template`'s own deep postcondition rejects only a `TypeParam` naming one of the
    generic's *own* parameters.

  Fn-call inference is partial by ruling and is not a `Sigma`; it feeds `bind_type_args` as a complete
  `Vec<Type>` with placeholders.
- **D2 — the shared resolver needs every constructor's declared parameters** (K1-D3, R14).
  `resolve_entity_name_with_shadowing` step 1 sees only structure names, and enums only as a names-only
  ambient set. α₁ adds one constructor table to S7 μ's `ResolutionCtx`: for each local and prelude structure
  and enum, its declared `TypeParamDecl`s (names, bounds, default `TypeExpr`s), built in
  `compile_builder/names_phase.rs` from the AST and the prelude templates. Defaults are resolved **on demand**
  at the use site, with the ctx the use site is resolved under, then handed to `bind_type_args` as
  `TypeParam`s — names_phase cannot resolve them (a default such as `= List<Bolt>` needs the full resolver).
  μ stays behaviour-preserving (S7 D-OOS-1); the behaviour change is α₁'s.
- **D3 — one arity message shape.** `E_TYPE_ARG_ARITY: {kind} '{name}' expects {N | R to N} type
  argument(s), got {G}`, `Diagnostic.code = DiagnosticCode::TypeArgArity`, primary label `'{name}' declares
  {total} type parameter(s)` at the use span. The CLI prints `{severity}: {message}` and has no code→mnemonic
  map, so the message carries the mnemonic prefix exactly as its sibling `E_TYPE_ARG_ON_TRAIT: trait 'X' …`
  already does; Rust tests assert the code by identity (INV-SF-6 house pattern), CLI signals the prefix.
  Traits keep `E_TYPE_ARG_ON_TRAIT` (compiler-type-hygiene §7.1) in the same shape with a `#5024` suffix.
  `{kind}` renders Alias→`type alias`, Structure→`structure`, Enum→`enum`, Builtin→`builtin type`,
  Trait→`trait`, TypeParam→`type parameter`, Dimension→`dimension`, Function→`function`; a `NAMED_DIMENSIONS`
  scalar name used as a type (`Length`) is Builtin, and Dimension is reserved for a dimension name written in a
  dimension slot.
- **D4 — `Orientation`/`Frame`/`Transform3` are arity 0** (R9). The vestigial `usize` is #6336's.
- **D5 — the builtin kind reads S8 ο's table.** `TypeArity::Fixed(row.arity)` before dispatch to
  `row.build`; the 24 hand guards and the silent `_ => None` die. If ο lands with hand-written arms, α₁ hoists
  one check above the `match` and strips the guards.
- **D6 — one instantiation pass** (R8). Binding (explicit, default, `auto:`) and instantiation are orthogonal
  axes (heuristic 3); the auto phase's purpose loses its "and" (heuristic 6); nested generics share one
  worklist and one mono-name dedup map (heuristic 11).
- **D7 — re-key through `InstancePath`** (R7). A re-key is a root rename (template-level ids and path-rooted
  override/auto ids), never `.entity.split`/`starts_with`; S3 OQ-2 b's typed `ValueCellId` stays open.
- **D8 — hydration never panics and never changes the CLI policy** (R10). `from_templates` has no diagnostics
  channel; the check runs in `Engine::eval` and `edit_source` beside `Snapshot::from_compiled_module`. On a
  valid program nothing changes; on a compiler-bug shape `reify check` exits 1 with a message through #5403's
  unchanged allowlist instead of panicking (debug) or staying silent (release).
- **D9 — `FnRef` is a signature key** (R5). An index cannot work: at `TraitMethodCall` lowering the callee
  does not exist yet (`phase_register_instance_assoc_fns` runs after `phase_entities`), and the two tables
  differ in order and length. `(name, params)` is unique by `phase_dup_sig_check` plus the shadow filter;
  `origin` is part of the key because a user fn shadowing a stdlib intrinsic must not be intercepted.
- **D10 — `content_hash` folds the callee always** (R6). S3 η #8677's "byte-identical" invariant is the
  node-hash *formula*; γ₁ moves one input once, deliberately. `topology_fingerprint` is unchanged.
- **D11 — erasure is preserved.** `type_args` is compile-time metadata (generic-user-functions D1/INV-2):
  eval never reads it and the hash does not fold it.
- **D12 — δ is where alias-body behaviour changes.** μ ships the context; δ moves alias bodies onto it. The
  alias-DFS pre-pass keeps its load-bearing deferral as the *context it builds* (μ's named empty-context
  constructor: empty structure/trait/enum sets), not as a separate resolver; `AliasBodyUseSite` is deleted.
- **D13 — the known-gap pin is replaced, not just kept.** "Turns green and is retired" means: the module
  `parametric_alias_deferred_alias_body_known_gap` is deleted and replaced by a parity module asserting
  `alias_ty == direct_ty && alias_errs.is_empty()` for its four rows plus #7573's rows.
- **D14 — deletion leaves carry deletion signals** (R11): γ₂ "eval output byte-identical, one resolver";
  β₂ "results byte-identical on the β₁ fixtures, one representability definition, the debug assert gone".
- **D15 — fixtures.** The `gicb_*` fixtures and `tooling_fe_unrepresentable_generic.ri` are committed PRD
  evidence. A leaf that makes a Rust test read one registers it in `scripts/verify.sh`'s
  `_RUST_COUPLED_RI_FIXTURES` in the same diff (PG-DRIFT, `tests/infra/test_verify_scope.sh`).

## §6 Contract (B+H)

Shared vocabulary. *Arity site*: a place where written type arguments meet a constructor's declared
parameters. *Instantiation site*: a `sub` whose target template is generic. *Type position*: an IR field
holding a `Type`.

### §6.1 α — `crates/reify-compiler/src/type_args.rs`

Crate-private, lowest stratum: imports only `reify_core::{Type, Diagnostic, DiagnosticCode, DiagnosticLabel,
SourceSpan}` and `reify_ir::TypeParam` (no `crate::` import, so it cannot join a module SCC; INV-ARCH-1).

```rust
pub(crate) enum TypeCtorKind { Alias, Structure, Enum, Builtin, Trait, TypeParam, Dimension, Function }
pub(crate) struct TypeCtor<'a> { pub kind: TypeCtorKind, pub name: &'a str }

/// How many type arguments a constructor takes.
pub(crate) enum TypeArity<'a> {
    Declared(&'a [TypeParam]),   // alias, structure, enum, generic fn (trait once #5024 lands)
    Fixed(usize),                // S8 ο's ParamBuiltin.arity; 0 for every bare name
}
impl TypeArity<'_> {
    pub fn total(&self) -> usize;
    pub fn required(&self) -> usize;   // 1 + index of the last default-less parameter; `total` for Fixed
}

/// Complete binding: exactly one Type per declared position, in declaration order.
/// Complete ≠ concrete (D1). No public constructor except bind_type_args.
pub(crate) struct Sigma<'a> { arity: TypeArity<'a>, args: Vec<Type> }  // args.len() == arity.total()
impl<'a> Sigma<'a> {
    pub fn args(&self) -> &[Type];
    pub fn get(&self, param: &str) -> Option<&Type>;      // Declared only
    pub fn as_map(&self) -> HashMap<String, Type>;        // the one bridge to substitute_type_params / unify
    pub fn is_concrete(&self) -> bool;                    // no TypeParam | ScalarParam descendant (S7 λ)
}

/// THE rule and THE diagnostic (D3): required <= got <= total, trailing positions from defaults.
pub(crate) fn bind_type_args<'a>(arity: TypeArity<'a>, args: Vec<Type>, ctor: TypeCtor<'_>, span: SourceSpan)
    -> Result<Sigma<'a>, Diagnostic>;
```

Invariants (heuristic 10):

- **INV-α1 one rule.** Every arity decision calls `bind_type_args`; it is the only constructor of
  `TypeArgArity`/`TypeArgOnTrait` diagnostics and of `Sigma`.
- **INV-α2 Applied is complete.** Every `Type::Applied { name, args }` has `args.len() ==` the declared total.
  Construction: A8 step 1 pads. Use: A4 and A9/A10 rebuild Σ through `bind_type_args`, so a short `Applied`
  is loud, never a silent zip.
- **INV-α3 informative.** The message names kind, constructor, accepted range and actual count (INV-2).

Site disposition:

| site | leaf | disposition |
|---|---|---|
| A1 | α₁ | `bind_type_args(Declared(alias.type_params), …, Alias, use span)`; `sigma.as_map()` replaces the inline substitution build; #9169's `alias_arity_accepts` is deleted (its test stays green: the message keeps `expects 1 type argument(s), got 2` and names `P`) |
| A2 | α₁ | calls `bind_type_args` anchored at `use_site.span` (#6477's rule); δ later deletes the arm with the twin |
| A3 | α₁ | `Declared(enum_def.type_params)` for both the bare case (`got 0`, accepted only when every parameter has a default — generic-enum R2(b)/R3) and the applied case; Err returns `Type::Error` (anti-cascade), never an erased `Type::Enum`; `enums_phase.rs`'s bare payload arm routes through it (#8098 position 1) |
| A3b | α₁ | deleted; the fallback calls `resolve_enum_type_with_args` |
| applied enum in the shared resolver (#8214) | α₁ | `resolve_entity_name_with_shadowing` gains the applied-enum arm: `bind_type_args(Declared(ctx.enum_type_params(name)), …, Enum, span)` → `Applied`, so an applied enum resolves at any depth and through alias bodies (the deferred non-parametric alias arm included); the five caller-level `resolve_enum_type_with_args` fallbacks (`entity.rs`, `functions.rs` ×2, `traits.rs`, `enums_phase.rs`) retire onto the shared path or narrow to its call; `defs_phase.rs`'s `resolve_enum_type` existence probe is unaffected |
| A4 | α₁ | arity half deleted (INV-α2); the bound loop iterates `bind_type_args(…)?.args()` |
| A5 | α₁ | calls `bind_type_args`; `sigma.as_map()` feeds the substitution |
| A8 step 1 | α₁ | `bind_type_args(Declared(ctx.structure_type_params(name)), …, Structure, span)` → `Applied { args: sigma.args() }` padded (K1-D3; defaults resolved on demand, D2); Err → `Type::Error` + the diagnostic |
| A8 step 2 (trait) | α₁ | `bind_type_args(Fixed(0), …, Trait, span)`; code and span unchanged |
| A8 step 3 (bare fallthrough with args) | α₁ | classify the bare name (builtin via the S8 ο table / `resolve_type_name`, type param, non-parametric alias, dimension) and `bind_type_args(Fixed(0), …)` → Err; `Orientation`/`Frame`/`Transform3` included (R9) |
| builtin guard arms (24) | α₁ | one `bind_type_args(Fixed(row.arity), …, Builtin, span)` before dispatch (D5) |
| def-site prefix zip | α₁ | iterates a `Sigma` |
| A6 | α₂ | arity/padding half replaced by `bind_type_args(Declared(type_params), …, Structure or Function, span)`; the bound loop iterates `sigma.args()` skipping `TypeParam`/placeholders; `effective_arg` dies |
| A7 | α₂ (seeding) / β₁ (pass) | the seeding copy and its "mirrors `effective_arg`" comment are deleted; Σ comes from `bind_type_args` in the instantiation pass; the out-of-range `clause.position` `continue` becomes unreachable and is deleted |
| A9, A10 | α₂ | `bind_type_args(Declared(enum_def.type_params), …, Enum, span)` → `sigma.as_map()` |
| `entity.rs` Named-flatten (×3), `convert_type_params` defaults (#8229) | α₂ | each written type argument goes through the full resolver, so `Holder<List<Bolt>>()` binds `List<Bolt>`, not `StructureRef("List")`; an unknown name in a default keeps the `StructureRef(name)` fallback |
| `type_compat::unify` `Applied` arm | — | out of scope: compares two complete `Applied`s (INV-α2 makes unequal arity a compiler bug); S7 λ D-OOS-2 keeps `unify` out of S7 too |
| `PendingBoundCheck::SubComponent { type_args }` | — | storage unchanged ("as written, resolved"); Σ is computed where the target's parameters are known (A6, β₁) |
| `TraitRef.type_args` on bounds | — | always empty; #5024's |

Generic functions: `instantiate_call_return_type` keeps `unify` into a map; γ₁ turns the result into
`type_args: Vec<Type>` in `type_params` order with `Type::unbound_placeholder(name)` for unbound positions
(R12 (1)); the fn bound check then calls `bind_type_args(Declared(f.type_params), type_args, Function, span)`.

**Ratchet rows** (`tests/infra/adoption-ratchet.manifest`, program INV candidate 5; the owning leaf adds its
row at the post-extraction count and names it in its done criteria; `path` is a pathlib glob, `**` supported):

| row | ERE | path | baseline at `baff3ed38b` | ceiling | owner |
|---|---|---|---|---|---|
| `type-arg-arity-site` | `\.len\(\) *(!=\|>\|<\|>=\|<=\|==) *[A-Za-z_.]*type_params\.len\(\)` | `crates/reify-compiler/src/**/*.rs` | 6 lines | 0 | α₂ (α₁ adds the row at its post-α₁ count) |
| `builtin-arity-guard-arm` | `" if type_args\.len\(\) == [0-9]` | `crates/reify-compiler/src/type_resolution.rs` | 24 | 0 | α₁ |
| `effective-arg-copy` | `let effective_arg:` | `crates/reify-compiler/src/**/*.rs` | 2 (the bare `let effective_arg` also matches `conformance/mod.rs`'s unrelated `effective_arg_type`) | 0 | α₂ |

The leaf re-measures each baseline before adding its row (S8 ο may lower `builtin-arity-guard-arm` first).

### §6.2 β — `compile_builder/monomorph.rs`, `template_is_concrete`, one instantiation pass, hydration

**The predicates** (R13, D1). Value representation is a property of a `Type`, so its one home moves beside
`Type`: reify-eval's `engine_eval.rs::is_representable_cell_type` (shallow: false for `TypeParam`,
`ScalarParam`, `Union`, `Keyed`, `Projection`) becomes `reify_core::Type::has_value_representation`, and eval's
users switch to it: the debug assert (until β₂), the `pub use` in reify-eval `lib.rs`, the LSP's
`diagnostics/eval_guard.rs`, `crates/reify-eval/tests/harness_tolerance/value_cell_type_invariants.rs`, the
reify-compiler `harness_auto_binding/auto_type_param_monomorphize_tests.rs` assertions and the
`is_representable_cell_type_*` unit tests (which move with it). β₁'s edit to `eval_guard.rs` meets S27 β #8761,
which moves that file: whichever lands second adapts (file locks; no edge, so G1 holds). Concreteness is a property of a
template, so it lives beside the struct it judges (heuristic 13): `pub fn template_is_concrete(t:
&TopologyTemplate) -> bool` and its witness `pub fn first_unrepresentable_template_cell(t) -> Option<&ValueCellDecl>`
in `crates/reify-compiler/src/types.rs` (or the child module that holds `TopologyTemplate` if `types.rs` is
split first), exported from the crate root for reify-eval. Definition: `t.type_params.is_empty()` and every
cell in `t.all_value_cells()` (S9 ν) and `t.ports[*].members[*]` has `cell_type.has_value_representation()`.
Placeholders inside derived types (`Applied{Result, [Int, __unbound_E]}`) are representable and legal.

**The extraction** (`crates/reify-compiler/src/compile_builder/monomorph.rs`, `pub(crate)`):

```rust
pub(crate) struct NotConcrete { pub template: String, pub cell: Option<ValueCellId>, pub ty: Type }

/// Clone `target` into a concrete template named `mono_name` under `sigma`.
/// Pre: sigma is Declared(&target.type_params) and sigma.is_concrete().
/// Post (both always-on; Err is reported as an internal-error Diagnostic, code
/// UnrepresentableCellType, and the sub is not rewritten):
///  (a) template_is_concrete(&out);
///  (b) substitution complete: no TypeParam/ScalarParam naming one of target's own parameters is a
///      descendant (Type::any_descendant, S7 λ) of any type position the destructure reaches —
///      today's top-level debug_assert made total. A function's own type_params are not target's.
pub(crate) fn instantiate_template(target: &TopologyTemplate, sigma: &Sigma<'_>, mono_name: &str)
    -> Result<TopologyTemplate, NotConcrete>;
```

The body opens with `let TopologyTemplate { name, doc, entity_kind, visibility, type_params, trait_bounds,
value_cells, constraints, realizations, sub_components, relations, ports, connections, guarded_groups,
structure_controlling, objective, meta, content_hash, is_recursive, annotations, pragmas, match_arm_groups,
forall_templates, assoc_fns, assoc_types } = target;` (every field at `baff3ed38b`; the leaf re-reads the
struct) and decides per field:

| field(s) | rule |
|---|---|
| `name` | `mono_name` |
| `type_params` | empty |
| `doc`, `entity_kind`, `visibility`, `trait_bounds`, `meta`, `is_recursive`, `annotations`, `pragmas` | clone |
| `value_cells` | id re-keyed; `cell_type` substituted; `default_expr` through `map_value_refs(rekey)` then result-type substitution; the zero-arg-ctor default synthesis for `param x : T` cells moves here unchanged |
| every `CompiledExpr` in `constraints`, `relations`, `ports`, `guarded_groups`, `objective`, `match_arm_groups`, `forall_templates` bodies, `sub_components.{args, pose, auto_pose.params, keyed_member_overrides}` | `map_value_refs(rekey)` + result-type substitution; nested `ValueCellDecl`s as `value_cells` |
| ids: constraint/realization ids, `guarded_groups[*].{guard_value_cell, parent_guard}` (`CompiledGuardedGroup`), `match_arm_groups[*].arms[*].guard_value_cell` (`GuardedDeclArm`), `count_cell`, `structure_controlling`, `connections.{compatibility_constraint, frame_constraint}` (`ConstraintNodeId`), `forall_templates.{parent_entity, count_cell}` | re-keyed |
| `sub_components.type_args`, `match_arm_groups[*].arms[*].arm_type`, `assoc_types.ty`, `assoc_fns.function.{params, return_type}` | substituted |
| `assoc_fns.function.body` | result-type substitution only (fn-scoped cell ids are not template-rooted); `phase_register_instance_assoc_fns` registers it under `mono_name::Trait::m` |
| `sub_components.structure_name` (rewritten by the pass), `keyed_members`, `connections` port paths (sub names) | clone |
| `content_hash` | `target.content_hash.combine(ContentHash::of_str(mono_name))` (today's rule) |
| `realizations` | every `GeometryOp` cell carrier re-keyed through reify-ir's op-level cell remap (enumerate the carriers at leaf time) |

`rekey(id)`: an id whose entity is `target.name` gets `mono_name`; an id whose entity is an instance path
rooted at `target.name` gets its root replaced. Implemented once, through S3 ι's `InstancePath` parse, root
replacement and `Display` (R7), never by string surgery on `.entity`. `rekey` is applied only through
`CompiledExpr::map_value_refs` (reify-ir), which already takes a `FnMut(ValueCellId) -> ValueCellId` and
rebuilds content hashes. `CompiledExpr::remap_entity` (in place, hash-stale by contract; callers
`post_passes.rs` and `engine_purposes.rs`) is not used for re-keying: it is the existing exact-entity special
case, and re-expressing it as `map_value_refs` with an exact-entity closure is out of scope (§13).

**The pass** `compile_builder/instantiate_phase.rs::phase_instantiate_generics(ctx, prelude)`, placed in
`lib.rs` after `phase_auto_type_param_resolution` (which now only writes `StructureRef` into `__auto_*`
positions) and before `phase_sub_override_autos`. A worklist over `(template, sub)` pairs in `ctx.templates`,
appending monomorphs as they are created. For each sub whose target (local or prelude) has non-empty
`type_params`:

1. `sigma = bind_type_args(Declared(target.type_params), sub.type_args.clone(), Structure, sub.span)`; Err →
   continue without pushing: a sub's arity diagnostic has one emitter, A6 (`check_type_param_bounds`, coded
   by α₂), so the pass never duplicates it.
2. `!sigma.is_concrete()` → continue: an enclosing generic's own parameter (instantiated when the enclosing
   template is), or a `__auto_*` left by a resolver failure (already an `E_AUTO_TYPE_PARAM_*` Error; #6851
   keeps fix-site-1 per C14). When the template is not itself generic and no Error was already recorded for
   this sub, push an internal-error diagnostic rather than skipping silently (INV-SF-3).
3. `mono_name = mangle_monomorph_name(&target.name, &mangled_args)` with `mangled_args =
   sigma.args().map(mangle_type_arg)`. `mangle_type_arg(&Type) -> String` is an **injective** encoding into the
   identifier alphabet: `Type::Display` emits `Scalar[Length]`, `Map<K, V>`, `Function(..) -> R` and
   `Base::member`, and S3 ι's `InstancePath` grammar uses `.`, `[` and quotes as delimiters. A structure argument
   encodes as its name, so today's auto names are unchanged (`Widget$SealA$G1`). Invariant: **a monomorph name
   is a valid `InstancePath` root segment**, pinned by a round-trip test over every `Type` Display shape. The
   existing collision guard applies.
4. First sighting → `instantiate_template` → push; every sighting → `sub.structure_name = mono_name`.
5. A self-growing generic (`A<T> { sub i = A<List<T>>() }`) is bounded: past depth 64 (the
   `MAX_ALIAS_INSTANTIATION_DEPTH` precedent) one Error `E_GENERIC_INSTANTIATION_DEPTH` names the chain.

One mono-name map serves auto-bound, defaulted and explicit subs.

**Hydration** (`crates/reify-eval/src/graph.rs`): `from_templates` skips every template with
`!template_is_concrete(t)` at the top-level loop and at child lookup (a sub still pointing at a generic is
#6851 shape 1 or a reported bind error; its cells are absent, never `TypeParam`-typed). A new
`pub fn non_concrete_template_diagnostics(templates: &[TopologyTemplate]) -> Vec<Diagnostic>` yields one Error,
code `DiagnosticCode::UnrepresentableCellType` (new, reify-core), for every template that claims concreteness
(`type_params` empty) yet fails the predicate, naming the witness cell: `template '{name}' is declared
concrete but value cell '{cell}' has type '{ty}', which has no value representation (INV-COMP-4)`. `Engine::eval` and `edit_source` call it
right after `Snapshot::from_compiled_module` and extend the result's diagnostics (D8).

**Invariant INV-COMP-4** (new row in `docs/invariants.md`, β₁'s diff): "Every template hydrated into the
evaluation graph is concrete: it declares no type parameters and every value cell's type has a value
representation (`template_is_concrete`, over `Type::has_value_representation`). Enforced at construction
(`instantiate_template`'s postcondition, plus its own-parameter substitution check) and at use (hydration skips
generic templates and diagnoses a concrete-claiming non-representable one)." One definition home, two
enforcement points, an informative message (heuristic 10).

**β₂ (follow-on, after #8761 and #8860): one predicate home.** β₁ already makes hydration skip generic
templates and diagnose the rest always-on, which by itself flips the LSP's compile-clean generic cases, so β₂
is a pure consolidation and deletion leaf (R4, R11):
- S27 β's `first_unrepresentable_cell` (in reify-eval after #8761; graph-level) is rewritten over
  `Type::has_value_representation` / `template_is_concrete`'s witness, or deleted if its LSP caller can read
  β₁'s hydration diagnostic instead (decide at leaf time). Either way there is one representability definition.
- `#[cfg(debug_assertions)] assert_value_cell_types_representable`, its two callers and its unit test are
  deleted: β₁'s always-on check subsumes them.
- The five stale `value_cell_type_invariants.rs` cites are repointed at INV-COMP-4 and the real test under
  `harness_tolerance/`.

**Supersession, in β₁'s diff:** a dated addendum to `docs/prds/v0_3/auto-type-param-resolution-completion.md`
§4.2 invariant 2 and §4.3: "Every sub whose target is generic is instantiated to a monomorph once its binding
is concrete — bound explicitly, by default, or by `auto:`. A module with no generic instantiation of any kind
produces zero monomorphs and leaves `ctx.templates` byte-identical; `topology_fingerprint` stability narrows
to that case. An uninstantiated generic template stays in `templates` and is not hydrated. §4.3's claim that
abstract templates never fire the representability invariant was false at main (#7225; this PRD §2.2)."

**Deletes:** `substitute_value_cell_collection`, `substitute_constraint_collection`, the clone-and-clear
block, the two coverage-rationale comment blocks and the top-level-only `debug_assert!` in the auto phase; the
insert-order masking (#7225); β₂ deletes the eval assert.

**Ratchet rows:**

| row | ERE | path | baseline | ceiling | owner |
|---|---|---|---|---|---|
| `monomorph-field-substitution` | `substitute_(value_cell\|constraint)_collection\(\|type_params\.clear\(\)` | `crates/reify-compiler/src/compile_builder/*.rs` | 11 lines | 0 | β₁ |
| `debug-only-representability-assert` | `assert_value_cell_types_representable` | `crates/reify-eval/src/**/*.rs` | re-measure at leaf time | 0 | β₂ |

### §6.3 γ — `UserFunctionCall { callee, type_args, args }`, `bind_user_call`, `FnTable`

**reify-ir**, new module `crates/reify-ir/src/fn_ref.rs` (heuristic 14: `expr.rs` is 3,373 lines):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FnOrigin { Local, Prelude }

/// The function a call binds to and the overload chosen (D9). Unique in the merged table by
/// construction: phase_dup_sig_check (key (name, param types)) plus the shadow filter. Covers free
/// fns, `Trait::f` statics and `Conformer::Trait::f` instance assoc fns (self param = StructureRef(conformer)).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FnRef { pub name: String, pub params: Vec<Type>, pub origin: FnOrigin }
impl FnRef {
    pub fn of(f: &CompiledFunction, origin: FnOrigin) -> FnRef;
    pub fn content_hash(&self) -> ContentHash;    // of_str(name) ⊕ each param ⊕ origin tag
}

/// One merged table, one shadow policy, one index (built once per compile and once per eval/edit).
pub struct FnTable { fns: Arc<[CompiledFunction]>, index: HashMap<FnRef, usize> }
impl FnTable {
    pub fn merge(user: &[CompiledFunction], prelude: &[CompiledFunction]) -> FnTable;  // today's merge_prelude_functions policy
    pub fn resolve(&self, r: &FnRef) -> Option<&CompiledFunction>;
    pub fn as_slice(&self) -> &[CompiledFunction];
}
```

`CompiledExprKind::UserFunctionCall { callee: FnRef, type_args: Vec<Type>, args: Vec<CompiledExpr> }`. One
constructor, `CompiledExpr::user_function_call(callee, type_args, args, result_type)`, and one hash fold:
`TAG_USER_FUNCTION_CALL ⊕ callee.content_hash() ⊕ args[i].content_hash…` (R6; `type_args` not folded, D11).
The compiler's `build_user_function_call_expr` and the reify-test-support builder call it and delete their
fold copies. S8 ζ's `intrinsic` field is deleted; its accessor becomes `fn intrinsic(callee: &FnRef) ->
Option<StdlibIntrinsic>` (`origin == Prelude` and the stdlib intrinsic table holds `(name, params.len())`),
and ζ's `classify_user_call` keeps its signature, fed by that projection.

**Compiler**, new module `crates/reify-compiler/src/call_binding.rs`:

```rust
/// A candidate the ladder may choose: from a CompiledFunction (sites 1, 2, 3, 5) or from a
/// CompiledAssocFnSig (site 4; key = FnRef { name: instance_assoc_fn_symbol(..),
/// params: [StructureRef(conformer)] ++ sig.params, origin }).
pub(crate) struct CallCandidate<'a> { pub key: FnRef, pub params: &'a [Type], pub return_type: &'a Type,
    pub type_params: &'a [TypeParam], pub param_defaults: Option<&'a [Option<CompiledExpr>]>,
    pub annotations: &'a [Annotation] }

pub(crate) struct CallBinding { pub callee: FnRef, pub type_args: Vec<Type>, pub result_type: Type,
    pub args: Vec<CompiledExpr> /* padded or selector-coerced */, pub deprecation: Option<String> }

pub(crate) enum Unbound { NoCandidates, Rejected(Diagnostic) }   // NoCandidates lets site 1 fall through to builtins

pub(crate) fn bind_user_call(candidates: &[CallCandidate<'_>], args: Vec<CompiledExpr>, call_name: &str,
    span: SourceSpan) -> Result<CallBinding, Unbound>;
```

Inside, in this order and nowhere else: wildcard tier (`slot_matches_wildcard_tier`) → exact tiebreak → head
tier (`slot_matches_head_tier`) → classify {1 → bound; >1 → Ambiguous; 0 → default padding → selector
coercion → NoMatch (with today's SelectorKindMismatch tagging)}; then `instantiate_call_return_type` (unify →
`type_args`, unbound → placeholder) and the deprecation lookup. All five sites call it; site 4 keeps its
`TraitMethodUnknown`/`TraitNotImplemented` pre-checks and gains the head tier (a correction, latent until
#5024). `type_compat.rs::OverloadResolution` folds into the classification.

`check_expr_fn_calls`: `functions.resolve(callee)`; trait-object conformance runs over `callee.params ⇄ args`
(so it now runs for site 4); the bound check runs when the callee has bounded `type_params`, via
`bind_type_args(Declared(f.type_params), type_args.clone(), Function, span)`. The re-resolve, the re-unify and
the stale comment are deleted. `ctx.resolution_functions` becomes an `FnTable`.

**Invariant INV-γ1:** the compiler's `bind_user_call` is the only place an overload is chosen; every consumer
reads `callee`.

**Eval (γ₂).** `EvalContext.functions: &FnTable` (every constructor in reify-eval/reify-expr/reify-constraints
changes type; enumerate at leaf time). `eval_user_function_call` resolves `ctx.functions.resolve(callee)`;
`None` is a compiler bug and records `UndefCause::UnboundCallee { callee }` when cause capture is attached
(INV-SF-1; today a cause-less `Undef`). `find_matching_compiled_function`, its doc and its ten callers'
re-selection are deleted (every caller holds a compiled call node, so none dispatches from a bare string);
`reify-eval/src/lib.rs::merge_functions` is deleted for `FnTable::merge` (one shadow policy); the reify-lsp
optimized-fn check reads `callee.name`. #5992's parity file, its `harness_langcore.rs` line and the
`__resolve_function_overload_for_parity_test` shim are deleted (R1).

**content_hash consequences (R6):** every `UserFunctionCall` hash moves once; `ValueCellNode` and
`ConstraintNodeData` hashes move for every cell/constraint containing a user call; persisted compute-cache
entries keyed through them recompute once; `topology_fingerprint` is unchanged.

**Ratchet row:** `eval-overload-reresolution | find_matching_compiled_function\( | crates/reify-e*/src/**/*.rs
| 0 | γ₂` — baseline 12 lines at `baff3ed38b`.

### §6.4 δ — `TypeParamEnv` on `ResolutionCtx`

After S7 μ and S8 ο:

```rust
pub(crate) enum TypeParamEnv<'a> {
    /// Inside a declaration: names resolve to TypeParam (and dim params to ScalarParam).
    Declared { type_params: &'a HashSet<String>, dim_params: &'a HashSet<String> },
    /// Inside an alias body being instantiated: a parameter name resolves to its binding.
    Bound(&'a Sigma<'a>),
}
// ResolutionCtx gains `type_params: TypeParamEnv<'a>` (replacing μ's two sets) and
// `use_site_span: Option<SourceSpan>` (the anchor #6477 established for alias-body diagnostics).
impl ResolutionCtx<'_> {
    /// Hygiene: REPLACES the environment — a body never sees the use site's parameter scope.
    pub(crate) fn with_bound<'b>(&'b self, sigma: &'b Sigma<'b>, use_site: SourceSpan) -> ResolutionCtx<'b>;
}
```

The one resolver gains a "parameter lookup" step in its Named arm: `Declared` → `TypeParam`/`ScalarParam`
leaf (args must be empty: `bind_type_args(Fixed(0), …, TypeParam, span)`); `Bound(sigma)` →
`sigma.get(name)`. `resolve_parameterized_alias` resolves the body with `ctx.with_bound(&sigma,
use_site).entering_alias(name)` and the ordinary entry, nothing else. The alias-DFS pre-pass keeps its
deferral as the context it builds (D12). The depth guard is replaced by μ's alias stack (a parametric
self-instantiation is a cycle by name). S8 ο's `Subst` was never a type on main; ο and δ agree to call the
binding `Sigma` (cross-reference line on #8903).

**Deletes:** `resolve_type_alias_expr_with_subst`, `resolve_parameterized_builtin_type_with_subst` and
`resolve_type_alias_expr_to_dim_with_subst` (if ο has not already merged the builtin twin), `AliasBodyUseSite`,
D13's `subst` parameter, the parity prose on `resolve_entity_name_with_shadowing`,
`MAX_ALIAS_INSTANTIATION_DEPTH`, and the known-gap pin module (D13).

**Ratchet row:** `alias-body-resolver-twin | resolve_type_alias_expr_with_subst\( |
crates/reify-compiler/src/type_resolution.rs | 0 | δ` — baseline 18 lines.

## §7 Boundary-test sketch

| seam | scenario | preconditions | postconditions |
|---|---|---|---|
| compiler→IR | explicit `Holder<GasketSeal>()` | generic `Holder<S: Seal>`; no `auto:` | `Holder$GasketSeal` in `templates`, `type_params == []`, `template_is_concrete`, cell `ValueCellId("Holder$GasketSeal", "seal")` with `cell_type == StructureRef("GasketSeal")`; the sub's `structure_name == "Holder$GasketSeal"`; generic `Holder` still present |
| compiler→IR | `Holder<Bolt>()` vs `Holder<Bolt, Steel>()` | `T: Rigid = Steel` | one monomorph `Holder$Bolt$Steel`, both subs point at it; `param h : Holder<Bolt>` is `Applied { Holder, [Bolt, Steel] }` |
| compiler→IR | probe (k) | `Widget<S, G>` with SealA/SealB | `Widget$SealA$G1` and `Widget$SealB$G1`, distinct `slot_t` ids; no id with entity `Widget` referenced by either |
| compiler→IR | nested generic `Outer<T> { sub i = Inner<T>() }`, `Outer<Bolt>()` | both generic | `Outer$Bolt` and `Inner$Bolt`; `Outer$Bolt.i.structure_name == "Inner$Bolt"` |
| compiler→IR | self-growing `A<T> { sub i = A<List<T>>() }`, `A<Int>()` | — | `E_GENERIC_INSTANTIATION_DEPTH` naming `A`; finite templates |
| compiler→eval | declared, never-instantiated `Bearing<T: Seal>` | zero diagnostics | no `Bearing.*` cell hydrated; `reify check`/`reify eval` rc 0; no debug panic; no `Undef` cell in release |
| compiler→eval | `auto:` failure (#6851 shape 1) | `E_AUTO_TYPE_PARAM_*` Error | sub still names the generic; hydration skips the child; no debug panic on the LSP/MCP path; cells absent; CLI exit 1 as today |
| compiler→eval | builder template, `type_params == []`, a `TypeParam` cell | reify-test-support builder | one `E_UNREPRESENTABLE_CELL_TYPE` Error naming template/cell/type, debug and release alike; no panic |
| compiler→eval | a generic structure with `let r = Ok(1)` (cell type carrying `__unbound_E`, generic-enum R1, after #8018), instantiated explicitly | β₁ | the monomorph hydrates and evaluates; no diagnostic (placeholders in derived types are representable, R13) |
| compiler→IR | `Holder<Length>()` (a non-nominal argument) | `Holder<T>` unbounded | one monomorph whose name contains none of `.`, `[`, `]`, quotes or spaces; every re-keyed id round-trips through `InstancePath` |
| compiler→eval | exit codes on the three framing fixtures | #5403 policy unchanged | rc 0; the only new exit-1 source is the compiler-bug diagnostic |
| compiler→eval (γ) | `pick(x: Int)` / `pick(x: Real)` | two overloads | `callee.params == [Int]` / `[Real]`; eval resolves the same declarations; values identical to main |
| compiler→eval (γ) | user fn shadowing a prelude fn | ζ's shadow warning | `callee.origin == Local`; `intrinsic(&callee) == None`; eval runs the user body |
| compiler→eval (γ) | `sc.(Weigher::weigh)(Bolt())` | `Scale : Weigher` | `callee.name == "Scale::Weigher::weigh"`, params `[StructureRef(Scale), TraitObject(Rigid)]`; conformance runs (the Foam variant errors); `FnTable::resolve(callee)` is `Some` at eval |
| compiler→eval (γ) | `@optimized` user fn cell | dispatcher registered | `is_optimized_userfn_cell` finds the target via `resolve(callee)` (esc-4093-152's class unreachable) |
| compiler→LSP | the declared-generic and explicit fixtures opened | β₁ (+ #8761 for β₂) | no `SkippedEval`; eval diagnostics flow |
| compiler→LSP | hover on `param h : Holder<Bolt>` | α₁ | renders `Holder<Bolt, Steel>` (R12 (2)) |
| compiler→compiler (δ) | `type Outer<T> = Option<Inner>` vs direct `Option<Inner>` | `Inner = Zq` | identical `Type`, empty diagnostics on both; `type W<U> = Zq<U>` and direct `Zq<Real>` print the same arity text |
| compiler→compiler (α₁) | `Option<Ge<Real>>`, `List<Ge<Real>>`, `type W = Ge<Real>` vs top-level `Ge<Real>` | `enum Ge<T>` | all four `Applied{Ge, [Real]}` (nested where written); no diagnostics |
| compiler→compiler (δ hygiene) | `structure def S<T> { param p : Outer<T> }`, `type Outer<U> = List<U>` | use-site scope has `T` | the body sees only `Bound{U → TypeParam(T)}`; a body naming `T` directly is `unresolved type` |
| snapshot/cache (γ) | compute cache persisted before γ₁ | entries keyed by hashes containing user calls | recomputed once, then hit; `topology_fingerprint` unchanged |

## §8 Cross-PRD relationship (G4)

| artefact | owner | this PRD's role | status |
|---|---|---|---|
| `type_args.rs` (`bind_type_args`, `Sigma`, `TypeArity`) | **this PRD** | owner; consumers: S8 ο's table (reader), #5024 (generic traits) | new G4 row (cross-reference line) |
| `compile_builder/monomorph.rs`, `phase_instantiate_generics`, the hydration-side check | **this PRD** | owner; consumers: S27 β #8761, #6851's route | new G4 row |
| `CompiledExprKind::UserFunctionCall` shape (after #8897) | **this PRD** (from γ₁) | S8 ζ hands over the variant; ζ's checks untouched | new G4 row |
| `ValueCellId` monomorph re-keying | **this PRD** | through S3 ι's `InstancePath`; S3 OQ-2 b's typed id stays S3's | new G4 row |
| `ResolutionCtx` | S7 μ #8643 | consumer: adds a constructor (structure + enum) type-param table (α₁) and `TypeParamEnv` (δ) | μ stays behaviour-preserving |
| `Type::children` / `any_descendant` | S7 λ #8638 | consumer | — |
| reify-builtins row schema / D13 `ParamBuiltin` | S8 ο #8903 | consumer (arity column); δ deletes `subst` | cross-reference line on #8903 (name the binding `Sigma`) |
| `intrinsic` stamp, reify_expr user-call dispatch with reify-stdlib | S8 ζ #8897 | γ₁ after ζ; owns the variant shape afterwards | — |
| `TopologyTemplate::all_value_cells()`, `MemberDecl` additions | S9 ν #8743 | consumer (β₁'s destructure) | — |
| `first_unrepresentable_cell` | S27 β #8761 | β₂ rewrites it over `Type::has_value_representation` or deletes it (follow-on) | — |
| `InstancePath` | S3 ι #8704 | consumer (β₁) | — |
| node-hash formula (#8677) / invalidation stratum | S3 η #8677 | γ₁ moves one input once (D10) | cross-reference line on #8677 |
| `reify_core::Diagnostic` codes (INV-SF-6) | S12 β #9024 | α₁, β₁ add codes in the existing shape | — |
| compile entry points | resolution-unification β #5516, S27 γ #8778 | unchanged front end | — |
| `docs/prds/v0_3/auto-type-param-resolution-completion.md` §4.2/§4.3 | that PRD (contract, shipped) | superseded by a dated addendum in β₁'s diff (R8) | — |
| `Orientation(usize)` family | #6336 (decision gate) | α₁ makes written args an error (R9); the type is #6336's | — |
| generic-enum arity, construction and match typing (`resolve_enum_type_with_args`, `variant_construct.rs`, the match arms in `expr.rs`) | generic-enum-type-arg-retention (#8013 α2, #8015 γ, #8018 δ, #8019 ε, #8020 ζ) | consumer: α₁ after #8013, α₂ after #8018; #8015/#8019/#8020 share files with α₂/γ₁ (file locks serialise; no edge) | edges wired; neither PRD widens the other |

No reciprocal "the other owns it" remains.

## §9 Decomposition plan

Fixture paths are `tests/prd-gate/fixtures/<name>.ri`. A leaf adding compile tests under
`crates/reify-compiler/tests/harness_langcore/` re-reads the harness root at leaf time
(`cargo-native-harness-layout` step A may `git mv` it to `harness_langcore/main.rs`). A leaf that moves a file
under PLINECITE scope runs `plinecite-baseline-gen --rename` in the same diff once S24 ο #8822 has landed; a
leaf that renames a symbol named by a `docs/invariants.md` pointer updates the pointer in the same diff (S24 ξ
#8821). New infra members are Python stdlib unittest with a thin `test_<name>.sh` wrapper and a
`run-all-classification.manifest` row.

| label | title | prio | depends on | user-observable signal |
|---|---|---|---|---|
| **α₁** #9358 | One type-argument binder: type_args.rs, padded Applied, coded arity diagnostic | **high**, gate #9171 | #8643, #8903, #8638, #9169, #8013 | `reify check gicb_alpha_default_padding.ri` → `All constraints satisfied.` rc 0 (today arity error); `gicb_alpha_nested_alias_arity.ri` → exactly one error, `E_TYPE_ARG_ARITY: type alias 'P' expects 1 type argument(s), got 2` (#9169 lands the naming code-less first; α₁ adds the code and shape), no `unresolved type: Q<Int>`; `gicb_alpha_nongeneric_builtin_args.ri` → five `E_TYPE_ARG_ARITY: builtin type '…' expects 0 type argument(s), got 1` errors, rc 1 (today clean); `gicb_alpha_nested_generic_enum.ri` → `All constraints satisfied.` rc 0 (today three `unresolved type`) |
| **α₂** #9359 | Bind type args at instantiation and pattern sites; delete effective_arg copies | **high**, gate #9171 | α₁, #8018 | `reify check gicb_alpha2_sub_arity_excess.ri` → `E_TYPE_ARG_ARITY: structure 'Holder' expects 1 to 2 type argument(s), got 3` (today code-less "too many …") |
| **β₁** #9360 | Extract monomorph.rs: instantiate_template, re-key, one instantiation pass, concrete hydration | **high**, gate #9171 | α₁, #8638, #8743, #8704 | `tooling_fe_unrepresentable_generic.ri` → `reify check`/`reify eval` rc 0 (today panic 101); `gicb_beta_explicit_instantiation.ri` → eval prints `Top.h.seal = GasketSeal { thickness: 2 }` (today panic); `gicb_beta_two_monomorph_collision.ri` → no `Widget.slot_t` line |
| **β₂** #9361 | One representability definition; delete the debug-only assert | **high**, gate #9171 | β₁, #8761, #8860 | deletion leaf (R11): `reify check`/`reify eval` and the LSP give byte-identical results on the β₁ fixtures; one representability definition remains (`Type::has_value_representation`); the debug assert and its callers are gone, and the five stale cites are repointed |
| **γ₁** #9362 | Record callee and type_args on UserFunctionCall; bind_user_call at all five sites | **high**, gate #9171 | α₁, #8897 | `reify check gicb_gamma_trait_method_arg_conformance.ri` → `error: type 'Foam' does not conform to trait 'Rigid' required by param 'other'` rc 1 (today clean, `Top.bad = 1`), matching the control `gicb_gamma_free_fn_arg_conformance_control.ri` |
| **γ₂** #9363 | Dispatch on callee at eval; delete find_matching_compiled_function and merge_functions | **high**, gate #9171 | γ₁, #5992 | deletion leaf (R11): `reify eval gicb_gamma_overload_dispatch.ri` prints `S.a = 1` and `S.b = 2`, byte-identical to main; ratchet `eval-overload-reresolution` at 0 |
| **δ** #9364 | Resolve alias bodies through the one resolver (TypeParamEnv on ResolutionCtx) | medium | α₁, #8643, #8903 | `reify check gicb_delta_alias_body_deferred_entity.ri` → `All constraints satisfied.` rc 0 (today `unresolved type: Outer<Real>`); `gicb_delta_alias_body_generic_enum.ri` (today `unresolved type: W<Real>`) may already pass after α₁ through the shared enum arm — δ keeps it passing on the one resolver and the parity module (D13) asserts equality |
| **ε** #9365 | Docs truth: defaults in annotations, explicit instantiation, one arity diagnostic | low | α₁, α₂, β₁, δ | the `types.md` / `structures.md` / `enums.md` chunks state the new behaviour and `harness_doc_chunks` is green; no `Orientation<3>` spelling remains in the docs |
| **ω** #9366 | PRD close: SHIPPED header, landed ids, AS-AUTHORED freeze | low | α₁–ε | the committed header |

**Edges onto other tasks:** `#9171 ← α₁, α₂, β₁, β₂, γ₁, γ₂` (survey gate; highs); `#8586 ← γ₂` (record
K1-D1); `#7882 ← β₁` (re-pointed before #6853/#7225 are cancelled). None onto any task in #8612's closure
(checked live 2026-10-18).

### Leaf detail

**α₁ #9358 — one type-argument binder** (x_finding_key fk-0e4c27b21db7, fk-6b74a4f8fcfc; x_finding_part
fk-4bd738a79576; high; gate #9171).

- §6.1: `type_args.rs`; the constructor (structure + enum) type-param table on `ResolutionCtx` (D2); sites A1,
  A2, A3, A3b, A4, A5, A8 (steps 1–3), the applied-enum arm (#8214), the builtin guard arms (D5) and the
  def-site zip.
- Absorbs **#7337** (bare builtin names; its rows in `type_resolution.rs::tests::orientation_annotations_compile_clean`
  that pin `Orientation<N>` as silently accepted flip to errors, R9), **#8098** (bare generic enum in a variant
  payload, position 1; position 2 is δ's through the one resolver), and **#9329** (K1-D3: the probe-(a) pair,
  `Holder<Bolt>` == `Holder<Bolt, Steel>` as Types, too few/too many still error; α₁'s compile test reads
  `gicb_alpha_default_padding.ri`), and **#8214** (R14: the applied-enum arm in the shared resolver; its
  measured position table — struct/fn/trait member/enum payload/constraint-def param × alias body and nested
  builtin argument — and #7057's `applied_enum_alias_body_known_gap` lock, which stays green and is then
  tightened to assert equality). Deletes #9169's
  `alias_arity_accepts` and keeps its test green. After #8013 (generic-enum α2) lands, its bare-generic-enum
  annotation helper is the A3 rule's interim home: α₁ routes it through `bind_type_args` and deletes the
  hand check (its spec §3.9.2 wording stands).
- Behaviour changes are §12 α₁; every test asserting an old arity message is updated in the same diff.
- Adds rows `type-arg-arity-site` (at its post-α₁ count) and `builtin-arity-guard-arm` (0).
- **Done when:** the three fixtures give the §9 outputs; `cargo test -p reify-compiler` green; the two
  ratchet rows are in the manifest and `python3 tests/infra/adoption_ratchet.py` is green; the fixtures any
  Rust test reads are registered in `_RUST_COUPLED_RI_FIXTURES` (D15).

**α₂ #9359 — instantiation and pattern sites** (x_finding_key fk-4bd738a79576; high; gate #9171).

- Sites A6, A7 (seeding copy), A9, A10, the `entity.rs` Named-flatten sites and `convert_type_params`
  defaults (§6.1). Absorbs **#8229** (cases `type L = Length; param b : L<Bool>`, `Scalar<Length<Bool>>` in an
  alias, `structure def D<T> { param a : T<Bool> }`; the flatten sites; overlap with #8151 noted there).
- File contention with γ₁ on `compile_builder/entities_phase.rs` (no edge; whichever lands second adapts).
  Runs after generic-enum δ #8018 (the `Applied` exits of `compile_variant_construct`); generic-enum ε #8019 and
  ζ #8020 edit the match arms near `binder_subst` (A10) and are serialised by file locks only.
- Takes `type-arg-arity-site` to 0 and adds `effective-arg-copy` at 0.
- **Done when:** `gicb_alpha2_sub_arity_excess.ri` gives the §9 output; `Holder<List<Bolt>>()`'s bound error
  names `List<Bolt>` (compile test); both rows at 0.

**β₁ #9360 — monomorph extraction** (x_finding_key fk-7bf9e6299eca; high; gate #9171).

- §6.2 entire except β₂: `Type::has_value_representation` (moved from reify-eval; S4 ν #8860 may first move
  `is_representable_cell_type` within reify-eval — file locks serialise, the second adapts),
  `template_is_concrete`, `monomorph.rs`, `instantiate_phase.rs`, `mangle_type_arg`, the auto phase reduced to
  binding, hydration skips + `non_concrete_template_diagnostics` + the new code, INV-COMP-4 in
  `docs/invariants.md`, the addendum to the auto-resolution PRD.
- Absorbs **#6853** (its `Bearing<GasketSeal>()` fixture and reproduction, the e2e acceptance mirroring
  `auto_resolved_param_produces_structure_instance`, its planned file list) and **#7225** (the uninstantiated
  generic fixture, the ValueCellId last-write-wins masking, candidate route (a) = skip generic templates at
  hydration, the regression-test requirement). Ports **#6851**'s evidence (ambiguous `auto:`, shape 2, the
  entry-point note) without cancelling it (R2); if #6851 has landed, β₁'s pass subsumes its hand fix.
- Adds row `monomorph-field-substitution` at 0.
- **Done when:** the three fixtures give the §9 outputs with no debug panic and no `Undef` in release; the
  §7 compiler→IR rows are compile tests; the builder-made non-concrete template yields the coded Error; the
  row is at 0.

**β₂ #9361 — one representability definition, debug assert deleted** (x_finding_part fk-7bf9e6299eca; high; gate #9171;
follow-on form under G1).

- §6.2's β₂ paragraph. After S4 ν #8860 the assert lives in `cell_type_invariants.rs`: re-read its home. The
  LSP `eval_guard` flips already happened in β₁; β₂ keeps them green.
- Adds row `debug-only-representability-assert` at 0.
- **Done when:** the deletion signal in §9 holds; `cargo test -p reify-eval -p reify-lsp` green; no
  `value_cell_type_invariants.rs` cite remains.

**γ₁ #9362 — call binding recorded** (x_finding_part fk-1b3f91469997; high; gate #9171).

- §6.3 except eval: `fn_ref.rs` in reify-ir; the variant change and its one constructor; `call_binding.rs`;
  all five sites; `check_expr_fn_calls` reads the binding; ζ's `intrinsic` field replaced by the projection;
  every matcher of the variant (reify-ir, reify-expr, reify-eval, reify-lsp, `gui/src-tauri`) reads
  `callee.name` in the interim; reify-expr's inline struct-literal tests updated; the hash pins
  (`builder_compiler_hash_agreement.rs`, the reify-ir fold pin) re-pinned deliberately (D10).
- **Done when:** `gicb_gamma_trait_method_arg_conformance.ri` gives the §9 output and its `Bolt()` sibling stays
  clean; `cargo test --workspace` green; `function_name: String` is gone from the variant.

**γ₂ #9363 — one resolver at eval** (x_finding_key fk-1b3f91469997; high; gate #9171; deletion leaf).

- §6.3's eval paragraph. Depends on #5992 so that its parity test and shim exist to be deleted (R1).
  `reify-expr/tests/find_matching_compiled_function_tests.rs` is rewritten against `bind_user_call` /
  `FnTable::resolve`. Adds row `eval-overload-reresolution` at 0. #8586 depends on γ₂.
- **Done when:** `gicb_gamma_overload_dispatch.ri` and the corpus evaluate byte-identically to main; the row
  is at 0; `UndefCause::UnboundCallee` is recorded for a builder-made unbound callee.

**δ #9364 — one TypeExpr resolver** (x_finding_key fk-7eb49b30a889; medium).

- §6.4. Absorbs **#7573** (gap 1 rows `Outer<T> = Inner` / `Option<Inner>` / `Option<InnerS>`; gap 2
  `W<U> = Ge<U>`; message parity `W<U> = Zq<U>`; the #6477 step-11 "not silently wrong" lock; its test list and
  memory hints). Replaces the known-gap pin module (D13). Takes #8098 position 2 (bare generic enum in an alias
  body) through α₁'s enum arm. Adds row `alias-body-resolver-twin` at 0.
- **Done when:** both δ fixtures give the §9 output; the parity module asserts equality and no errors on
  every row; the row is at 0.

**ε #9365 — docs truth** (low; consumes every behaviour leaf).

- `crates/reify-mcp/src/tools/chunks/types.md` ("Type Parameters": defaults fill omitted trailing arguments in
  every position, `Holder<Bolt>` ≡ `Holder<Bolt, Steel>`; one arity diagnostic; `Orientation` / `Frame` /
  `Transform3` take no type arguments), `structures.md` (explicit instantiation `sub h = Holder<GasketSeal>()`
  instantiates exactly like `auto:`), `enums.md` (the bare-generic-enum message), plus the 48 doc lines spelling
  `Orientation<3>`, `Frame<3>`, `Transform<N>` and the like in `docs/reify-stdlib-reference.md`,
  `docs/reify-language-spec.md` and the chunks (R9; the spec's structural `Frame<N>`/`Transform<N>`
  description is reworded to the N=3 types with a pointer to #6336), plus the same spellings in user-visible
  code strings: `crates/reify-lsp/src/completion.rs` (completion signatures) and
  `crates/reify-eval/src/geometry_ops.rs` (runtime "not a valid Orientation<3>/Transform<3>" warnings, strings
  and comments; tests pinning those strings move with them) and `crates/reify-compiler/src/geometry.rs` (the
  "expects target + a List<Transform<3>>" diagnostic). Each documented signature compiles as written in a smoke `.ri`. Discoverability: an author asking "can
  I leave out a defaulted type argument" or "why is my generic sub not instantiated" finds it in
  `types.md`/`structures.md`. No new authoring idiom, so no `examples/best_practices/` entry and no reify-design
  index line.

**ω #9366 — PRD close** (low). Depends on every other leaf; a cancelled leaf counts as satisfied. Stamps the
terminal status, names the landed leaf ids, adds the AS-AUTHORED freeze paragraph and LIVE/AS-AUTHORED map
(§6 stays LIVE: rustdoc on `type_args.rs`, `monomorph.rs` and `fn_ref.rs` will cite it), and applies the
matching header to the capability manifest.

## §10 Finding-key disposition

| key | survey id | severity | leaf | how |
|---|---|---|---|---|
| fk-4bd738a79576 | compiler-type-substitution.1 | high | α₁ (part), α₂ (key) | one binder; every arity site calls it |
| fk-7bf9e6299eca | compiler-type-substitution.2 | high | β₁ (key), β₂ (part) | instantiate_template, re-key, INV-COMP-4 at construction and use |
| fk-1b3f91469997 | compiler-type-substitution.3 | high | γ₁ (part), γ₂ (key) | callee recorded once; eval re-resolution deleted |
| fk-7eb49b30a889 | compiler-type-substitution.4 | medium | δ (key) | one TypeExpr resolver |
| fk-0e4c27b21db7 | compiler-type-substitution.5 | medium | α₁ (key) | absorbs #9329 (K1-D3 interim) |
| fk-6b74a4f8fcfc | compiler-type-substitution.6 | low | α₁ (key) | #9169 lands first (interim); α₁ deletes its helper |

## §11 Absorption map (port before cancel; `x_subsumed_by`)

| task | disposition | into | ported content |
|---|---|---|---|
| #7337 | cancel | α₁ | the 12-name table, the discard site, the `Orientation<N>` docs lines, the pinned test rows |
| #8098 | cancel | α₁ (position 1), δ (position 2) | three positions, measured payload case, test cases (a)–(c) |
| #9329 | cancel | α₁ | the probe-(a) pair, four work items, its delivered check |
| #8214 | cancel (R14) | α₁ | the measured position table, root cause, #7057's `applied_enum_alias_body_known_gap` lock |
| #8229 | cancel | α₂ | three fixtures, the flatten-site list, the #8151 overlap |
| #6853 | cancel | β₁ | fixture, reproduction, e2e acceptance, file list |
| #7225 | cancel | β₁ | fixture + panic text, masking, routes (a)–(c), regression requirement |
| #7573 | cancel | δ | gap 1/2 tables, message parity, step-11 lock, tests, memory hints |
| #6851 | **kept** (C14, R2) | — | evidence ported into β₁ |
| #9169 | **kept** (lands first) | — | α₁ deletes `alias_arity_accepts` |
| #5992 | **kept** (lands first, R1) | — | γ₂ deletes its test and shim |

#7882 (milestone, outside the closure) depends on #6853 and #7225; β₁ is added to its dependencies before
either is cancelled.

## §12 Behaviour changes, stated as corrections

- **α₁:**
  - `param h : Holder<Bolt>` with a defaulted `T` compiles.
  - One arity message shape with `E_TYPE_ARG_ARITY` replaces the alias shape (code-less; `type_alias_compile_tests.rs`, #9169's test keeps its substring), the enum `expects N … found G` and `does not accept type arguments` shapes (`generic_enum_pattern_binder_tests.rs`, `enum_generic_construction_inference_tests.rs`, `struct_ctor_field_conformance_tests.rs`, `tests/prd-gate/generic-enum-type-arg-retention-alpha2-probe-set.json` whose `expects 2 type argument` substring survives; `generic-enum-type-arg-retention.md` R2 quotes the old text), the structure `wrong number of type arguments` shape (`type_arg_applied_resolution_tests.rs`, `parametric_vector_point_resolution_tests.rs`, `coupling_motionvalue_integration_gate.rs`, `diagnostic_coverage_checkpoint.rs`, `m9_error_cases.rs`), and the projection shape (code `UnresolvedType` → `TypeArgArity`).
  - `E_TYPE_ARG_ON_TRAIT` keeps its code and span; its text takes the unified shape with the `#5024` suffix (`trait_type_arg_rejection_tests.rs`).
  - Newly rejected: `Int<Real>`, `Length<Int>`, `Bool<Length>`, `String<Int>`, `T<X>`, a non-parametric alias with args, dimension names with args, and `Orientation<N>` / `Frame<N>` / `Transform3<N>` (R9; `orientation_annotations_compile_clean`'s `<N>` rows flip).
  - The nested-alias arity error is loud and the `unresolved type: Q<Int>` cascade disappears.
  - A padded `Applied` prints padded in every diagnostic and hover (R12 (2)); no test at main declares a defaulted structure type in an `Applied` literal (verify at leaf time).
  - A bare generic enum in a variant payload field now errors (#8098 measured zero corpus uses).
  - Newly accepted: an applied enum in nested position and in alias bodies (`Option<Ge<Real>>`, `List<Ge<Real>>`, `type W = Ge<Real>`; #8214); a non-generic enum given args (`Zq<Int>`) gets the unified arity error at every depth.
- **α₂:** `too many type arguments …` and `missing type argument for type parameter …` take the unified shape with a code (`trait_bounds_tests.rs`, `m9_error_cases.rs`); nested sub type args resolve fully; `convert_type_params` defaults naming parametric types resolve instead of flattening.
- **β₁:** the three framing fixtures stop panicking (debug) and yielding `Undef` (release); a generic template's own cells disappear from `reify eval` output (no `Widget.slot_t` line) and monomorph cells carry `$` names; `topology_fingerprint` / `ctx.templates` move for every module with an explicit generic sub (R8); after an `auto:` failure the sub's cells are absent instead of `TypeParam`-typed; an out-of-range `auto:` clause position is an arity error instead of a silent skip; a wrong-arity generic sub is not instantiated and keeps exactly one diagnostic (A6's).
- **β₁ (continued):** a compiler-bug template shape gets one Error in release and debug; the LSP no longer reports `SkippedEval` for compile-clean generic shapes (`eval_guard.rs` cases, `compile_clean_unrepresentable_graph_*` tests flip to "evaluates"); a non-nominal explicit argument (`Holder<Length>()`) gets an identifier-safe monomorph name.
- **β₂:** none on valid programs (consolidation and deletion).
- **γ₁:** a trait instance-method call's trait-object arguments are checked (newly rejected: the Foam case); site 4 gains the head tier (latent until #5024); every `UserFunctionCall` content hash moves once (pins re-pinned; persisted cache entries recompute once); IR `Debug` dumps of calls change shape.
- **γ₂:** no value change on valid programs; an unbound callee at eval (a compiler bug) records `UndefCause::UnboundCallee`; `find_matching_compiled_function_tests.rs` and `prelude_backed_harness_tests.rs` are rewritten; #5992's parity file is deleted.
- **δ:** newly accepted: `type Outer<T> = Inner | Option<Inner> | Option<InnerS>` (and `type W<U> = Ge<U>`, if α₁'s enum arm has not already made it pass); `type W<U> = Zq<U>` reports the direct spelling's arity text; `E_BARE_SCALAR` fires inside alias bodies (non-pub ones are newly caught); a bare generic enum in an alias body is rejected; the self-instantiating parametric alias message becomes μ's cycle diagnostic.

## §13 Out of scope

- Typed `ValueCellId` (S3 OQ-2 b, post-S1): β₁ re-keys at the string level through `InstancePath` only.
- SO(2) / `Orientation(2)` and the vestigial `usize` on `Frame`/`Transform`/`AffineMap` (#6336).
- Generic traits and trait type arguments (#5024): `TypeArity::Declared` for traits is one arm when it lands.
- Retyping `substitute_type_params` / `unify` to take `&Sigma` (S7 λ D-OOS-2 keeps the transforming walkers
  out of S7; this stream bridges with `Sigma::as_map`).
- Monomorphising generic user functions: generic-user-functions D1 erasure stands.
- The CLI exit policy (#5403; S27 OQ3-A).
- The compiler-root poison of a failed `auto:` resolution (#6851, C14).
- Re-expressing `CompiledExpr::remap_entity` as `map_value_refs` with an exact-entity closure.
- Generic assoc fns: a trait method naming its own type parameter is `unresolved type in trait` today (#5024);
  `template_is_concrete` reads only cell types, so a function's own `type_params` never count against a
  template, and β₁'s deep postcondition names only the template's own parameters.

## §14 Open questions (tactical)

1. **Code names** `E_UNREPRESENTABLE_CELL_TYPE` and `E_GENERIC_INSTANTIATION_DEPTH`: no collision at
   `baff3ed38b` (generic-enum R4 reserves only `E_ENUM_TYPE_ARG_UNRESOLVED`). Decide in β₁.
2. **`FnRef::content_hash` encoding:** `Display(param)` (what cells use) vs a byte encoding. Padded `Applied`
   Display is stable after α₁. Decide in γ₁.
3. **`SubComponentDecl.content_hash`:** whether it folds the mono name (today it folds the written decl; the
   template's hash folds the name). Decide in β₁.
4. **`OptimizedNameIndex`** in `unfold/optimized_instance_reuse.rs`: keep as a name pre-filter or drop. Decide
   in γ₂.
5. **`g1`-shape double emission:** `weigh(Foam(), 2.0)` reports the bound error twice today (two walkers).
   Fix in α₂ or γ₁, whichever lands second.
6. **Where `template_is_concrete` lives** if `types.rs` (2,606 lines) is split before β₁: beside
   `TopologyTemplate`, wherever that is.
7. **γ₁ split.** γ₁ touches reify-ir, ~22 compiler files and every matcher in five crates; it is landable as
   one leaf because interim matchers read `callee.name`. If the architect finds it too large, split coherently:
   γ₁a = `FnRef`/`FnTable`/variant/constructor/matchers with sites passing `FnRef::of(resolved, origin)`;
   γ₁b = `bind_user_call` at the five sites + `check_expr_fn_calls` (the Foam signal lands there).
8. **`mangle_type_arg` grammar** (β₁): any injective identifier-alphabet encoding that keeps structure names
   verbatim; pinned by the round-trip test.
