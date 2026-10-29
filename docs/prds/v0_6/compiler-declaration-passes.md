# PRD: Compiler declaration passes — one classification per member, one acceptance relation

**Status:** active — authored 2026-10-12 (pass 1 of the `/prd` program for stream **S9** of
`plans/review-all-program-reify-20261012.md`); decomposed 2026-10-12 (pass 2): α #8730 … π #8746,
λ #8741 held on S6; gap-fill 2026-10-16: ρ #9321, σ #9322, ε₂ #9323; retro-critic sitting
2026-10-20 (R4, R6): D1, D2, D5, D7, D9, D11, D12, §3, §5, §6, §7 and §8 amended in place (see
§"Retro-critic sitting amendments (2026-10-20)"); esc-8739-2 (2026-10-29): D7, §6 B11 and §8 ι
amended (see §"esc-8739-2 amendment (2026-10-29)"). Shape: **B+H** (contract §5,
boundary-test sketch §6). Milestone: v0_6.

**Code anchors** verified against main `8e6ffcac9d` (2026-10-12); the review record's anchors were
taken at `a030ee7528` (2026-10-10) and `git diff a030ee7528..main` over every path below shows no
production-source change. Main moves fast: cite by symbol and re-locate lines at implementation time.

**Finding keys owned (15):** fk-0bdf9598b402, fk-196deab564e7, fk-30505ec518b4, fk-ee1be91862ca,
fk-27489259525d, fk-60053402aba5, fk-6621fbaa0058, fk-cf6356374fa1, fk-254bc351a8ab,
fk-37775e022693, fk-cc676a29f5d9, fk-75d9313c6ee6, fk-cb941b3d058e, fk-76bdd62534fe,
fk-ddfdce77efbc. Statements, measurements and skeptic notes: `plans/review-all-reify-20261010.json`.
§9 maps every key to exactly one leaf or one "not addressed because" line.

---

## §1 Goal

The compiler's declaration passes answer the same questions in several places, and the answers can
disagree:

- what kind a `let` is (geometry, selector, geometry list, rejected list, value) is decided again by
  each pass;
- whether a member kind is compiled, rejected, or silently dropped is decided separately in each
  member context;
- several closed sets travel as strings (reflective query kinds, deferred forall-connect endpoints,
  the connector-sub marker, parse-error categories);
- argument acceptance has two homes, and the trait-default skip policy is four parallel sets.

When this PRD lands, each of those questions has one answer in one place, the disagreements users
can see today are gone, and adding a member kind, a let shape, a relation or a reflective query is
one edit the compiler checks for exhaustiveness. Heuristics applied: 11 (SPOT), 10 (invariants
enforced uniformly), 12 (structured data instead of meaningful strings), 3 (one mechanism per axis
of variability), 9 (deep modules with narrow interfaces), 13/14 (files that make sense alone; no
file too large).

**User-observable surface** (each item is a leaf signal in §8):

1. A geometry alias declared before its referent gets the same diagnostics as one declared after it
   (`tests/prd-gate/fixtures/decl_passes_forward_geometry_alias.ri`, §2.1).
2. A member written where the compiler cannot honour it produces a diagnostic. It is never dropped
   silently (`decl_passes_guarded_port_dropped.ri`, `decl_passes_purpose_body_sub_swallowed.ri`,
   §2.2).
3. A deferred-count `forall … : connect` with a literal `coll[0]` endpoint connects the same ports
   as the resolved path, when the count arrives through `Engine::edit_param` (the GUI parameter
   path).
4. Parse errors carry a kind, so a front end maps them to a `DiagnosticCode`, and the
   member-continuation discriminator compares data instead of message text.
5. The remaining leaves preserve behaviour. Their signals are byte-identical `reify check` output on
   named fixtures, plus a ratchet or a type that makes the old pattern unrepresentable.

## §2 Background: what the code does at `8e6ffcac9d`

### §2.1 Let classification is order-sensitive, and the passes disagree (fk-0bdf9598b402)

`compile_entity` (entity.rs) keeps `known_geometry_lets`, `known_selector_lets`,
`known_geometry_list_lets` and `rejected_geometry_list_lets` as `HashSet<&str>` locals across three
passes. Pass 1 fills them incrementally, so each let is classified only against the names declared
before it. Pass 2 (the main member loop) and pass 3 (realization emission and its
`geometry_realization_member_name` closure) call `is_geometry_let` again against the completed sets.
The guarded-member helper `collect_geometry_exprs`, `guards.rs::register_guarded_names` and
`compile_guarded_members`, and the mirror `build_structure_def_skeleton` do the same.
There are 17 call lines of `is_geometry_let` / `classify_geometry_list_let` / `is_selector_expr` in
entity.rs and guards.rs.

Probe (debug `reify` built 2026-10-11, compiler source unchanged since `a030ee7528`):

| fixture | `reify check` |
|---|---|
| `decl_passes_backward_geometry_alias.ri` (`let base = box(…)` then `let shifted = base`, `constraint shifted > 0`) | rc 1, `error: comparison '>' left operand must be a comparable kind, got 'Geometry'` |
| `decl_passes_forward_geometry_alias.ri` (same members, `shifted` declared first) | rc 0, `INDETERMINATE … operator undefined for these operand kinds: GeometryHandle` |

Pass 1 registers the forward alias at the placeholder `Type::dimensionless_scalar()`, so the
constraint type-checks. Passes 2 and 3 then compile `shifted` as geometry, and the constraint becomes
indeterminate at eval. The pinned `let_scope_tests::cyclic_ident_alias_does_not_crash` covers only
the cyclic case. The function doc still says "Two-pass compilation".

### §2.2 Member support is decided per context, and some drops are silent (fk-196deab564e7)

`MemberDecl` (reify-ast `decl.rs`) has 19 variants. Seven hand-written matches decide what each one
means in their context:

- the entity member loop (entity.rs);
- the port-body member loop inside it, which ends in `_ => {}`;
- `guards.rs::register_guarded_names` and `compile_guarded_members`;
- `traits.rs::compile_trait`, which ends in `_ => { // Minimize, Maximize, GuardedGroup — skip }`;
- `compile_purpose`, top level;
- `unsupported_purpose_member_info`, the purpose guarded block.

Measured:

- `compile_guarded_members` no-ops Port, Connect, Chain, AssociatedType, Fn, MetaBlock,
  ConstraintInst, and MatchArmDeclGroup (handled by the parent). The grammar's `commonMembers` admits
  port, connect, chain, meta_block and constraint_instantiation inside `where { }`. Probe:
  `decl_passes_guarded_port_dropped.ri` → rc 0, "All constraints satisfied.", no diagnostic for the
  port or the meta block.
- `compile_purpose` carries 11 rejection arms whose strings duplicate
  `unsupported_purpose_member_info` verbatim. It also no-ops Relate, Sketch and Fn at top level.
  `purpose_member` (grammar.js) admits only constraint, let, minimize, maximize, guarded_block,
  default, pragma and structure, so none of those arms is reachable from source.
- **Premise correction:** what makes those arms unreachable is that the source is swallowed. A `sub`
  in a purpose body yields an ERROR node in tree-sitter. `ts_parser.rs::lower_purpose_members` reads
  only `purpose_member` children, so the ERROR child is skipped with no `ParseError`. Probe:
  `decl_passes_purpose_body_sub_swallowed.ri` → `tree-sitter parse` rc 1 (ERROR on the `sub` line),
  while `reify check` gives rc 0, "All constraints satisfied." Sibling bodies (`lower_constraint_def`)
  turn an `"ERROR"` child into `push_fault_error_with_excerpt`. The purpose body does not. So
  `compile_builder/sketch_unsupported.rs`'s rule ("every position that admits a sketch block must
  reject it") is broken today wherever the parser swallows the block. Deleting the 11 arms is
  correct only together with that parser fix (§8 γ).
- Corpus check: no tracked `.ri` file puts port, meta, connect, chain or a constraint instantiation
  inside a `where` block (scan of `git ls-files '*.ri'`, 2026-10-12). Turning those drops into Errors
  breaks no corpus file.

### §2.3 String-carried closed sets (fk-30505ec518b4, fk-ee1be91862ca, fk-27489259525d, fk-76bdd62534fe, fk-ddfdce77efbc)

- **Reflective purpose queries.** `ResolvedSchemaQuery.query_kind` (reify-compiler types.rs) and
  `CompiledExprKind::PurposeReflectiveAggregation.query_kind` (reify-ir expr.rs) are `String`. The
  per-kind cell filter exists twice: `compile_purpose`'s resolved-queries loop, and the fallback in
  `engine_purposes.rs::expand_purpose_reflective_placeholders`. The fallback's `else` arm returns
  `Vec::new()` for an unknown kind, which gives a vacuous-true `forall`. The struct doc advertises a
  `"ports"` kind that nothing handles. The IR content hash folds `ContentHash::of_str(&query_kind)`.
- **Deferred forall-connect endpoints.** `elaborate_forall_connect` flattens each endpoint through
  `resolve_port_name` into `CompiledForallBody::Connect.left_port_template` /
  `right_port_template: String`. `engine_edit.rs::rewrite_port_placeholder` materialises element `i`
  with `replacen("<sub>[0]", "<sub>[i]", 1)`. Its own doc concedes that a user-written `vents[0].foo`
  becomes `vents[i].foo` on the deferred path but not on the resolved path. Five `Diagnostic::info`
  strings in forall_elaborate.rs call tasks #2629, #2690 and #2717 "future scope", and all three are
  done. The module doc still says the Connect body is unsupported at runtime.
- **Connector sub-components.** `connect.rs` mints `format!("__connector_{}", …)`. Three
  `starts_with("__connector_")` recognisers repeat the literal:
  `engine_eval.rs::connector_pin_if_determined`, `is_strict_connector_instance_auto`, and
  `resolve_order.rs`'s connector-first edge loop. `SubComponentDecl` has no origin field. A user may
  name a sub `__connector_0`: probe rc 0, no reserved-name diagnostic. A CLI probe could not make
  that name change a solver outcome, so ζ's signal is structural (§8).
- **Parse errors.** `reify_ast::ParseError` is `{ message, span }`. `member_continuation.rs` exports
  `is_member_continuation_message`, a substring test whose own doc says "text matching is the wrong
  shape". It has 4 consumer test files in 3 crates. `module_dag.rs` maps a `ParseError` to
  `Diagnostic::error(e.message.clone())` and drops the span.

### §2.4 Conformance: four skip sets, a second acceptance home, phase-pinned tests (fk-cf6356374fa1, fk-37775e022693, fk-254bc351a8ab)

- `conformance/checker.rs::PreRegisterOutput` carries `pass1_skipped`, `pass1_param_skipped`,
  `pass2_skipped` and `pass2_compile_errors`. `check_trait_conformance` threads all four into
  `check_phase_build_available_defaults_map` and `check_phase_inject_defaults`, and each phase
  re-implements the guard ladder, including the `cell_type.is_some()/is_none()` conjunctions. The
  set-to-guard table exists only as doc prose on `PreRegisterOutput`.
- `conformance/mod.rs::walk_param_against_arg_type` (444 lines, 316 of them comments) defers to
  `type_compat.rs::type_compatible` in 4 arms. Its TraitObject, Vector, Point, Matrix/Tensor and
  Field arms carry their own rules, some deliberately different: the Vector arm accepts any
  `Tensor { rank: 1, .. }` regardless of `n`. The differences are explained in comments that cite
  type_compat.rs by line.
- The conformance test module has 79 `#[test]`s. Its test region makes 19 direct
  `check_phase_*(…)` calls plus 19 `check_fn_arg_conformance(…)` calls. The module doc says
  "All four tests in this module…" and sends the reader to "the escalate_info record for task 2033".

### §2.5 Joint self-check and relation tables (fk-cc676a29f5d9, fk-cb941b3d058e)

- `joint_self_check.rs` exports five `pub(crate)` pieces plus `DofKinds`. The gate that makes the
  verdict sound (skip on a `Type::Error` DOF; `E_ARG_TYPE_MISMATCH` and skip on an unclassifiable
  DOF; skip on an undecidable rot/trans split) lives in `compile_builder/entities_phase.rs` §(4). Two
  module docs hand that duty to "the caller".
- `relation_signatures.rs` spreads each relation's facts over `RELATION_FN_NAMES`,
  `is_relation_shared_verb`, `relation_fn_result_type`, `relation_delta_dof`,
  `relation_delta_dof_kinds`, `relation_metric_slot`, `relation_operand_datum` and
  `check_relation_arg_types`. The arity-3 policy is encoded as 10 `args.len() == 3` guards. The
  constant's doc says "**9 names**" and the slice holds 10, because `fasten` was added. The
  ΔDOF-sum test iterates its own `curated` list.

### §2.6 h14 measurements (record `h14_measurements`)

- **entity.rs**: 8,264 lines, `stays_whole: false`, 7 partitions. One is the AST walkers. Another is
  `compile_entity`, whose cost is h4/h5 inside the function. Another is match-arm grouping: reach-back
  through `use super::*`. Another is the skeleton: h11, deleted by #5494. The last three are
  cell/bound helpers, objective/annotation checks, and `mod tests`.
- **conformance/mod.rs**: 8,753 lines, `stays_whole: false`, 6 partitions. The walker and the
  geometry leaf form a cycle (`walk_param_against_arg` ↔ `check_leaf_trait_conformance`), so
  `WalkCtx` and `diag_at` must move to a lower module first. The other partitions are arg
  conformance, joint bound, the orchestration root and `mod tests`.

## §3 Pre-conditions and already-filed owners

| owner | status at authoring | relation to this PRD |
|---|---|---|
| #5425 uniform-member-access β (type-driven geometry-position acceptance; reworks "geometry.rs geometry-let allowlist + wildcard"; first production consumer of `member_path::resolve_member_path`) | pending, dispatched | **β depends on it.** β builds the classifier on the post-#5425 predicates. |
| #5430 uniform-member-access η (routes sub member access through the resolver; retires expr.rs `template_has_member` / `port_member_is_priv` and the `"count"` arms) | pending | **Owns fk-6621fbaa0058** (§9). |
| #5426 uniform-member-access γ (a top-level geometry-typed alias `let cut = c.body` is itself a geometry let and realizes) | pending | **[Amended 2026-10-20, retro-critic R4]** **Depends on β** (`#5426 ← #8731`). It adds the `MemberKinds` variant `TypedGeometryAlias`, written by a NAMED refinement step after pass-2 typing (§4 D1). Files: entity.rs, geometry.rs. |
| #5428 uniform-member-access ε (fn returning Geometry) | pending | **[Amended 2026-10-20, retro-critic R4]** **Depends on β** (`#5428 ← #8731`); it changes the let classification, so it lands over the classifier. |
| #5494 stdlib-namespace γ (authoritative templates before fn bodies; deletes `build_structure_def_skeleton`) | pending | β is order-independent of it (§4 D1). ξ does not move the skeleton group. **[Amended 2026-10-20, retro-critic R4]** Deleting the skeleton removes 5 of R1's calls: under α's two-sided ratchet (§4 D12) #5494 lowers R1 by the calls it deletes, in the same diff. |
| #6669 geometry-algebra-solver-unification β (`LoweringTable`, `Row.codim`) | pending | **[Amended 2026-10-20, retro-critic R4]** **Depends on λ** (`#6669 ← #8741`); it derives the codim over λ's arity-from-row form and keeps R3 at λ's committed value, or lowers it in the same diff (§4 D9, D12). **[Amended 2026-10-20, retro-critic R6]** Its LoweringTable keys on `reify_builtins::RelationKind`. |
| #8071, #8195, #8202, #8203 (specialization-body silent drops: let, constraint, lowering of spec-body constraints, connect/chain/forall/relate) | pending | **[Amended 2026-10-20, retro-critic R4]** **Consumers of γ's `member_support` table** (§4 D2). #8071, #8195 and #8203 depend on γ and add `SpecializationBody` table rows plus `unsupported_member_diagnostic`, with no per-kind module and no `E_SPEC_BODY_*` codes unless the table carries them; #8202 consumes the `SpecializationBody` row. |
| #5499 stdlib-namespace ζ (declared `import` headers for all stdlib modules; strict per-module compile; ordering comments deleted; NS-M3 determinism test) | pending; deps #5494, #5496 (done), #5497, #5498 | **Owns fk-60053402aba5** (§9). It is strictly stronger than the review's remedy. |
| #8556 (mechanical: correct `ambient_defaults.rs` module doc) | pending | **Owns fk-75d9313c6ee6** (§9). Not subsumed by any leaf here. |
| S7 (explicit `use crate::…` imports replace the crate-root globs and `use super::*`) | wave 1, earlier in the compiler order | ξ and ο depend on its leaves for entity.rs and conformance (§7). |
| S6 (~~`RelationKind` enum in reify-ir, parsed once by the compiler~~ (superseded by R6)) | wave 1, spawn | **[Amended 2026-10-20, retro-critic R6]** S6 γ #8830 binds `reify_builtins::RelationKind` (exhaustive `describe_demand` / `residual_dispatch`); λ depends on #8830 (§7). |
| S8-ψ #9418 (builtin registry: `BindingKind::Relation`, the 12 relation rows minting `RelationKind`) | pending | **[Amended 2026-10-20, retro-critic R6]** **λ depends on it** (§4 D9, §7). |

No novel grammar. Every fixture in this PRD parses with the current tree-sitter grammar, except
`decl_passes_purpose_body_sub_swallowed.ri`, whose grammar rejection is the point (§2.2).
`grammar_confirmed = true` for every leaf.

## §4 Resolved design decisions

**D1. LetKind is computed once, by a stratified, order-free classifier (β).** A new module
`let_kind.rs` exposes:

```
classify_members(members: &[MemberDecl], functions: &[CompiledFunction], resolved_param_types) -> MemberKinds
```

**[Amended 2026-10-20, retro-critic R4]** (M6) Pass 1 resolves each param's type first (through the alias, assoc-type, template and
trait registries, as today), and the classifier takes those resolved types as an input: a
geometry-defaulted param counts only when its resolved type is `Type::Geometry`. The classifier
returns `GeometryList(shape)` only after a pure element-cap check; it emits no diagnostics. Pass 1
emits the geometry-list diagnostics (today `diagnose_unsupported_geometry_list` and the
`expand_geometry_list_elements` over-cap `None`) from the classifier's verdict, so the cap decision
has one home.

It computes selectors to a fixpoint first, because `is_selector_expr` depends only on the selector
set. It then computes geometry to a fixpoint given the final selector set, because `is_geometry_let`
is monotone in the geometry set once the selector set is fixed. It then classifies geometry lists
and rejected lists given both sets. Everything else is `Value`. A forward alias is therefore
classified the same as a backward one. A cycle (`let a = b; let b = a`) is the least fixpoint, so
neither let is geometry, as `cyclic_ident_alias_does_not_crash` pins today.

`MemberKinds` maps every declared let and geometry-defaulted param occurrence to a `LetKind`, and
covers members inside guarded `where`/`else` arms. The variants are Geometry, Selector,
GeometryList(shape), RejectedGeometryList, CrossSubGeometryAlias and Value.

**[Amended 2026-10-20, retro-critic R4]** (B3) `MemberKinds` is written by the classifier and by NAMED refinement steps, each with a
stated input. These are the only exceptions to the purity contract (§5 row 1):

1. `classify_members` — the pure, order-free fixpoint over the members, `functions` and the resolved
   param types (above).
2. The cross-sub refinement — `is_bare_cross_sub_geometry_alias` needs the sub types that pass 1
   registers (input: pass-1 sub types). It runs once immediately after pass-1 sub registration and
   writes `CrossSubGeometryAlias` into the same table. Pass 2 treats that variant as a value cell
   and pass 3 treats it as a realization, which is exactly today's split, now named.
3. The typed-alias refinement — #5426 (uniform-member-access γ, which depends on β) adds the variant
   `TypedGeometryAlias` for a top-level geometry-typed alias such as `let cut = c.body`, whose
   classification needs `c`'s structure type (input: pass-2 let types). It runs once after pass-2
   typing and writes into the same table.

A later shape that needs scope adds a further named step with a stated input; no pass re-runs a
let-kind predicate (ratchet R1 counts every such predicate by name list, §4 D12).
Every reader after the classifier reads `MemberKinds`: pass 1's scope registration, pass 2, pass 3,
`collect_geometry_exprs`, `register_guarded_names`, `compile_guarded_members`, and
`build_structure_def_skeleton` if it still exists when β lands.

β therefore does not wait for #5494. The skeleton calls the same function, and #5494 deletes that
call later. The pre-classification hoist predicate `is_hoistable_query_call` keeps its direct
syntactic call, because it runs before any member is classified. It is the one named exception in
ratchet R1.

**D2. One member-support table, no silent disposition (γ).** A new module `member_support.rs`:

```
enum MemberContext { EntityBody, PortBody, GuardedBlock, TraitBody, PurposeBody, PurposeGuardedBlock, SpecializationBody, MatchArm }
enum MemberSupport { Compiled, HandledBy(Owner), Rejected }
fn member_support(kind: MemberKindTag, ctx: MemberContext) -> MemberSupport
fn unsupported_member_diagnostic(kind: MemberKindTag, ctx: MemberContext, span: SourceSpan) -> Diagnostic
```

**[Amended 2026-10-20, retro-critic R4]** (B2) `SpecializationBody` (`sub x : T { … }`, which the grammar admits `_member` into; today it
silently drops constraint, connect, meta and let) and `MatchArm` (where
`compile_match_arm_decl_group` already rejects non-Sub members) are member contexts like the others,
and both join B2's matrix (§6). The pending specialization-body tasks consume this table rather than
growing a second home: #8071 (let), #8195 (constraint) and #8203 (connect/chain/forall/relate)
depend on γ and add `SpecializationBody` rows plus `unsupported_member_diagnostic` (no per-kind
module, no `E_SPEC_BODY_*` codes unless the table carries them); #8202 is the consumer of the
`SpecializationBody` row. INV-SF-3 is enforced for that context by the same table.

`MemberKindTag` is a fieldless mirror of `MemberDecl`'s variants, produced by one exhaustive
`MemberDecl::tag()`. `HandledBy` names the pass that consumes the kind in that context, for example
MatchArmDeclGroup inside a guard → the parent entity loop, or AssociatedType in an entity body →
conformance. There is **no** `Ignore` variant. INV-SF-3 (`declared-intent-consumed-or-diagnosed`)
is the floor, and this table is how it is enforced for member positions.

A kind the grammar excludes from a context still gets `Rejected`, because hand-built ASTs reach the
compiler in tests. Its message comes from the one diagnostic function, which replaces the 11
duplicated strings and `unsupported_purpose_member_info`. Every member loop consults the table in
its fallthrough, so a 20th `MemberDecl` variant is one row per context, checked exhaustively in one
match.

In the same leaf, `lower_purpose_members` reports an ERROR child as a `ParseError`, as
`lower_constraint_def` does. Whether the remaining constrained bodies have the same hole is measured
in-leaf against the boundary matrix (§6 B2) and fixed if so.

**D3. `ReflectiveQueryKind` lives in reify-ir, and the matcher lives where `ValueCellKind` lives (δ).**
`enum ReflectiveQueryKind { Params, GeometricParams, MaterialParams }` goes in reify-ir, with
`as_str()` returning today's spellings and `from_member_name(&str) -> Option<Self>`. It is parsed at
the compiler boundary that `PURPOSE_REFLECTIVE_AGGREGATION_MEMBERS` and
`determinacy_intrinsic_member` guard today, and both return the enum. `ResolvedSchemaQuery` and
`CompiledExprKind::PurposeReflectiveAggregation` carry the enum.

One function decides membership:

```
reflective_query_admits(kind: ReflectiveQueryKind, cell_kind: &ValueCellKind, cell_type: &Type) -> bool
```

It sits in reify-compiler beside `is_geometric_param_type` / `is_material_param_type`. Both
`compile_purpose` and the `engine_purposes` fallback call it, and the fallback becomes an exhaustive
match with no unknown-kind arm. The content hash folds `ContentHash::of_str(kind.as_str())`, which is
byte-identical to today's input, so persisted cache keys do not move. The `"ports"` doc claim is
deleted. Adding a fourth kind is a deliberate edit to the enum, and the compiler then shows every
site.

**D4. `PortPath` replaces the endpoint template strings (ε).**

```
CompiledForallBody::Connect { left: PortPath, right: PortPath, … }
PortPath { root: PortRoot, members: Vec<String> }
PortRoot { Element, Literal { sub: String, index: Option<i64> }, Named(String) }
PortPath::at_element(&self, coll_sub: &str, i: i64) -> String
```

`Element` is the bound variable's element. `at_element` is the only materialiser, and it returns
exactly the dotted-bracket port-name string that `CompiledConnection.left_port`/`right_port` carry
today. Typing those fields is out of scope (§10). `rewrite_port_placeholder` is deleted. A literal
`vents[0]` is `Literal { index: Some(0) }` and never changes. The task citations leave the Info
messages, and the module doc states the current runtime behaviour.

**D5. `SubOrigin` on `SubComponentDecl` (ζ).** The new field is
`origin: SubOrigin { Declared, Connector { index: u32 } }`. connect.rs sets it at the mint. The
three recognisers look the sub up by name in `template.sub_components` (as
`connector_pin_if_determined` already does) and match on `origin`. The name mint stays: instance
paths are still strings, and typing them is S3's `normalize_cell_id` finding. The prefix becomes one
`pub(crate) const` in connect.rs, and no other site spells it.

**[Amended 2026-10-20, retro-critic R4]** (M2) The `__connector_` prefix is reserved. A user sub or member named `__connector_*` is an
Error (or the mint skips taken names): with the name mint and a by-name lookup, a user
`sub __connector_0` in a structure that also has a connector `connect` collides with the minted name
and silently changes the connector's solved value (probe: 10mm becomes 3mm). Boundary row B9b (§6)
pins it. "Only connect.rs constructs `Connector`" has one exception: the reify-test-support builders,
which must build `SubOrigin::Connector` for the resolve_order.rs and
`auto_binding_sites_remaining_resolution.rs` test fixtures.

**D6. `DefaultDisposition` replaces the four skip sets (θ).** `check_phase_pre_register_default_types`
returns one `DefaultDisposition` per `(name, AvailableDefaultKind)`. The variants are Inject,
SlotTakenByAnnotatedLet, SlotTakenByParam, SlotTakenByUnannotatedLet and CompileError (working
names). `PreRegisterOutput::disposition(name, kind)` is the single accessor. Both phases match on it
exhaustively, and the `cell_type` conjunctions disappear because the kind is in the key. The
set-to-guard prose table is deleted. The forward-reference limitation of unannotated lets is
unchanged (§10).

**D7. `conformance_accepts` is one named relation beside `type_compatible` (ι).** A new module
`type_acceptance.rs` defines
**[Amended 2026-10-20, retro-critic R4]** (M1)
`conformance_accepts(param: &Type, arg: &Type, registries) -> ArgVerdict { Accept, Reject, Unverifiable, QuantityConflict { param_q, arg_q }, RequiresTraitConformance(trait) }`
and imports `type_compat::type_compatible` downward. It takes the registries (`enum_defs` at least,
for the enum arm). `QuantityConflict` carries the Vector/Point/Matrix quantity-slot rejection and its
own message; `RequiresTraitConformance` hands a TraitObject parameter back to the walker, which keeps
the TraitObject recursion (the relation never calls conformance, so no
type_acceptance ↔ conformance module cycle forms). The type is named `ArgVerdict` to avoid
`reify_ir::arg_acceptance::Acceptance`. It does not live in type_compat.rs: that file is
5,299 lines, and S7/S8 partition it. Each deliberate divergence from `type_compatible` is an
explicitly named arm with a one-line pointer to its rationale in the language spec or this PRD. The
Vector-accepts-any-`Tensor<1>` rule is one example. The walker keeps only wrapper recursion (the
TraitObject recursion included) and diagnostic emission. Every site that reaches the walker (a
struct-ctor argument, and a fn-call argument whose parameter carries a trait object) gets its
verdict from the relation, so those sites cannot disagree.
**[Amended 2026-10-29, esc-8739-2]** That is NOT yet true of every acceptance site, and ι
does not make it so. Two known gaps remain, both owned by #9571, which depends on ι:
(1) a fn-call argument at a concrete (non-trait-carrying) parameter is judged by
`reify_core::overload`'s exact slot matching, not the relation; (2)
`check_param_default_conformance` checks only StructureRef and Geometry cells and leaves every
other default unchecked. The relation is still the one place where the verdict for each row is
decided. The arm owners edit its arms (#8121 Point scalar, #8317 Matrix/Tensor scalar, #8101
Geometry, #9568 quantity slot, #9569 Field erasure, #9570 Matrix nested-list erasure), and #9571
then routes fn-call and default through it, with no fn-call verdict loosening except on the rows
ruled Unverifiable.

**D8. One gated DOF entry point (κ).**
`joint_dof_diagnostics(joint_name, dof: &[(name, Type, span)], body: &[CompiledExpr], span) -> Vec<Diagnostic>`
owns both gradualism gates and the verdict. The pieces become private, and tests drive the entry
point.

**D9. One `RelationSig` per relation kind, over the registry's `RelationKind` (λ).**
**[Amended 2026-10-20, retro-critic R6]** Relation-signature facts have one home, the builtin registry. `RelationKind` IS
reify-builtins' generated `Relation` binding-kind sub-enum, minted by `registry!` from the 12
relation rows that S8-ψ #9418 adds (`EnumIter`, matched exhaustively by its owning crate per I-REG-2).
λ's table is a total

```
fn relation_sig(kind: RelationKind) -> RelationSig { operands: OperandPolicy, metric_slot: Option<MetricSlot> }
```

in relation_signatures.rs: relation semantics only. Arity and result type are read from the
reify-builtins row (`reify_builtins::row_for(BuiltinId::from(kind))`), never re-declared, so a new
relation row is a compile error in this match until λ's operand policy is written. There is no
`RELATION_FN_NAMES` and no other name-keyed table: names derive from the rows, and
`lookup(name, argc)` is the one parser (I-REG-1; no `from_name`). `is_relation_typed_fn` and
`is_relation_shared_verb` keep their `pub` signatures and derive from the rows (a shared verb is a
`Relation` row whose name also has a non-`Relation` row at another arity); `relation_metric_slot`
and `relation_operand_datum` derive from `relation_sig`; the tests iterate `RelationKind::iter()`.
The 10 `args.len() == 3` guards become one `row.arity.matches(args.len())`.

~~A static table holds `{ kind: RelationKind, arity: Arity, operands: OperandPolicy, metric_slot }`.
It is keyed by S6's `RelationKind`. `RELATION_FN_NAMES`, `is_relation_typed_fn`,
`relation_metric_slot`, `relation_operand_datum` and the arity-3 policy all derive from it, and the
tests iterate it.~~ (superseded by R6)

**[Amended 2026-10-20, retro-critic R4]** (M4) λ also replaces the arity guards inside `relation_delta_dof` and
`relation_delta_dof_kinds` (5 of R3's 10) with the row arity, values unchanged; that is how R3
reaches 0. #6669 depends on λ (`#6669 ← #8741`) and derives the codim over that arity-from-row
form.

*Lead amendment 2026-10-13 (codim seam, raised by S6's session):* the codimension has ONE home,
geometry-algebra-solver-unification β #6669's `LoweringTable` (`Row.codim`, keyed by
(RelationKind, DatumKind, DatumKind)), which that earlier PRD already owns together with the measured
`individual_rank` half. `RelationSig` therefore carries no `codim` field. ~~λ leaves
`relation_delta_dof(_kinds)` computing as they do today, keyed on `RelationKind`;~~ (superseded by R4
M4: λ replaces their arity guards with the row arity, values unchanged) #6669 derives them
from `Row.codim`. B13's rot + trans == ΔDOF clause moves to #6669's B4 ("the compiler's published
codim and the solver's codim are the SAME FIELD").

~~Builtin result typing and arity are also S8's reify-builtins row facts. λ keeps them in the row only
as relation semantics ("which overload is a relation"), and S8's rows read `RelationSig` rather than
re-declaring it (cross-reference §7).~~ (superseded by R6, which supersedes program pass-1 ruling S9
Q3(a) "S6 mints RelationKind; S9 λ owns the RelationSig rows; S8 reads them": reify-builtins is a
leaf crate and can read nothing in reify-compiler or reify-ir.) **[Amended 2026-10-20, retro-critic R6]** λ reads S8-ψ's rows;
the direction is λ ← S8-ψ (cross-reference §7). `units.rs::datum_constructor_result_type`'s arity-2 `offset`
form stays where S7 moves the builtin tables.

**D10. `ParseErrorKind` (μ).** `ParseError { message, span, kind: ParseErrorKind }`, where
`ParseErrorKind` is `Syntax | MemberContinuationAmbiguity`, plus kinds that existing distinct
`push_error` families justify, decided in-leaf. `ParseErrorKind::diagnostic_code()` is the single
kind → `DiagnosticCode` mapping. The code spellings are working names (`E_PARSE_SYNTAX`,
`E_MEMBER_CONTINUATION`). `module_dag.rs` maps through it and keeps the span.
`is_member_continuation_message` is deleted, and its four consumers compare `kind`.
`ParsedModule::render_errors` output stays byte-stable.

**D11. `TopologyTemplate::all_value_cells()` (ν).** It returns `value_cells`, then every guarded
group's `members` and `else_members`. Guarded groups are stored flat, with `parent_guard`, so no
recursion is needed. It is the G4 seam S9 owns for S27's finding fk-b420a7695f68. ν routes the
reify-compiler chains through it. **[Amended 2026-10-20, retro-critic R4]** S4 μ #8870 routes the reify-eval chains (8) and the
reify-test-support chain (1) (#8870 ← #8743, #8730); S27 routes the reify-lsp chains, eval_guard
included. Ratchet R4 counts what is left.

**D12. One construct-ratchet member (α).** `tests/infra/test_compiler_decl_pass_ratchets.py` (stdlib
`unittest`) has a thin `test_compiler_decl_pass_ratchets.sh` wrapper and a
`run-all-classification.manifest` row in the same diff. It counts code constructs (comment lines
excluded) against committed baselines, and hermetic fixtures prove that each count both flags and
clears. That makes it a construct lint, not a prose pin (tests/infra/README.md "Assert behaviour,
not source prose"). The member asserts no wall-clock bound.

**[Amended 2026-10-20, retro-critic R4]** (M5) Every row is **two-sided**: a count above its baseline reds, and a count below its
baseline is STALE and also reds, with the message to lower the row; the diff that removes counted
constructs lowers the row in the same diff. (This differs deliberately from the ceiling-only
`adoption_ratchet.py`.) Non-S9 tasks that move a count carry a same-diff note: #5494 lowers R1 by
the skeleton calls it deletes, and #6669 keeps R3 at λ's committed value or lowers it. (B3) R1
counts calls of every let-kind predicate by a NAME LIST (today the three below), so a newly named
predicate added to a pass is caught.

| id | construct | scope | baseline at `8e6ffcac9d` | target | lowered by |
|---|---|---|---|---|---|
| R1 | call of any let-kind predicate on the row's name list: `is_geometry_let(` / `classify_geometry_list_let(` / `is_selector_expr(` | entity.rs, guards.rs | 17 | 1 (`is_hoistable_query_call`) | β (#5494 lowers by the skeleton calls it deletes) |
| R2 | `"__connector_` string literal | `crates/*/src` production code (excluding `#[cfg(test)]` modules) | 4 | 1 (the const) | ζ |
| R3 | `args.len() == 3` | relation_signatures.rs production region | 10 | 0 | λ (`relation_delta_dof(_kinds)` included; #6669 keeps or lowers) |
| R4 | inline `guarded_groups` → `members`/`else_members` value-cell union outside `all_value_cells` | `crates/*/src` production code | measured by ν | reify-compiler 0; reify-eval and reify-test-support S4 (#8870); reify-lsp S27 | ν, then S4 and S27 |

Members of extraction leaves name their ratchet row (INV candidate 5, extract-with-ratchet).
Leaves whose superseded pattern becomes unrepresentable by type need no row: δ (String gone),
ε (template strings gone), θ (sets gone), μ (substring predicate gone).

**D13. Already-owned remedies are not re-filed.** fk-60053402aba5 → #5499, fk-6621fbaa0058 → #5430
(with #5425), and fk-75d9313c6ee6 → #8556 (§9). Filing duplicates would fight those tasks for the
same files, which contract §8 step 2 exists to prevent.

## §5 Contract (B+H)

| seam | producer | consumers | invariant |
|---|---|---|---|
| `let_kind::classify_members` → `MemberKinds` | β | compile_entity passes 1–3, guards.rs, `collect_geometry_exprs`, the skeleton while it exists; #5426 (`TypedGeometryAlias` step) | **[Amended 2026-10-20, retro-critic R4]** Order: pass 1 resolves param types first; `classify_members` is a pure function of the member list, `functions` and those resolved param types; then the named refinement steps write into the same table — `CrossSubGeometryAlias` after pass-1 sub registration (input: pass-1 sub types), `TypedGeometryAlias` after pass-2 typing (input: pass-2 let types; #5426) — and no other writer exists (§4 D1). Independent of declaration order; every let and geometry-defaulted param occurrence has exactly one `LetKind`; `GeometryList(shape)` only after the pure element-cap check, and pass 1 emits the list diagnostics from that verdict; no reader re-runs a let-kind predicate (R1, by name list). |
| `member_support(kind, ctx)` + `unsupported_member_diagnostic` | γ | every member loop over `MemberDecl` in reify-compiler, specialization bodies and match arms included; `MemberDecl` variant additions (reify-ast); #8071, #8195, #8203 (`SpecializationBody` rows), #8202 | Exhaustive over `MemberKindTag × MemberContext`; `Rejected` always produces an Error-severity diagnostic naming the kind and the context; **[Amended 2026-10-20, retro-critic R4]** every MemberDecl loop, specialization bodies (`SpecializationBody`) and match arms (`MatchArm`) included, consults the table, so no context drops a member silently and no second member-support home exists. |
| `lower_purpose_members` ERROR handling | γ | `reify check`, LSP, GUI | An ERROR child of a purpose body yields one `ParseError` at its span. |
| `ReflectiveQueryKind` + `reflective_query_admits` | δ | compile_purpose, reify-ir IR node, engine_purposes fallback, content hash | No String kind remains; `as_str()` spellings and hash bytes unchanged; the fallback match is exhaustive. |
| `PortPath::at_element` | ε | engine_edit.rs per-element materialisation | Output for an `Element` root equals the resolved path's port name for element `i`; a `Literal` index is never rewritten. |
| `SubComponentDecl.origin` | ζ | engine_eval.rs (2 recognisers), resolve_order.rs | Only connect.rs constructs `Connector` (**[Amended 2026-10-20, retro-critic R4]** except the reify-test-support builders, which build it for test fixtures); recognisers never inspect the name; a user sub or member named `__connector_*` is an Error (or the mint skips taken names). |
| `PreRegisterOutput::disposition` | θ | `check_phase_build_available_defaults_map`, `check_phase_inject_defaults` | Both phases consult one value per `(name, kind)`; advertising and injection cannot disagree. |
| `type_acceptance::conformance_accepts` → `ArgVerdict` | ι | conformance walker; fn-call, ctor and default sites | One relation; divergences from `type_compatible` are named arms; **[Amended 2026-10-20, retro-critic R4]** it takes the registries and never calls conformance (a TraitObject param yields `RequiresTraitConformance`, recursed by the walker); a quantity-slot disagreement is `QuantityConflict`. |
| `joint_self_check::joint_dof_diagnostics` | κ | entities_phase.rs | The only `pub(crate)` item of the module; owns both gates. |
| `relation_sig(RelationKind)` | λ | relation_signatures resolvers (`relation_delta_dof(_kinds)` included), reify-lsp hover (`is_relation_typed_fn`, `is_relation_shared_verb` keep their `pub` signatures); #6669 | **[Amended 2026-10-20, retro-critic R6]** One row per `RelationKind`; operand policy and metric slot derive from it; arity and result type are read from the reify-builtins row (S8-ψ #9418), never re-declared; no name-keyed table. ~~One row per `RelationKind`; every per-relation fact derives from it.~~ (superseded by R6) |
| `ParseErrorKind::diagnostic_code` | μ | module_dag.rs; S27 `compile_user_module`, reify-lsp `convert_parse_error`; S25 GUI diagnostics | One mapping; render output unchanged. |
| `TopologyTemplate::all_value_cells` | ν | reify-compiler sites; S27 | value_cells ∪ guarded members ∪ else_members, in that order. |

## §6 Boundary-test sketch

| # | scenario | preconditions | postconditions | leaf |
|---|---|---|---|---|
| B1 | forward vs backward geometry alias | `decl_passes_{forward,backward}_geometry_alias.ri` | Both `reify check` → rc 1, identical error line; `cyclic_ident_alias_does_not_crash` green. | β |
| B1b | forward selector alias | let `s = t` before `let t = faces(b)`, used as a selector arg | Same compile result as the backward order. A postcondition guard, not a RED premise: the forward selector order was not probed on main. | β |
| B2 | member-support matrix | every `MemberDecl` kind the grammar admits in each `MemberContext` (**[Amended 2026-10-20, retro-critic R4]** `SpecializationBody` and `MatchArm` included), as `.ri` source through the compile entry (not hand-built ASTs) | Each cell is compiled, or produces exactly one Error naming kind and context; zero silent cells. | γ |
| B3 | guarded port and meta | `decl_passes_guarded_port_dropped.ri` | rc 1; one Error for the port, one for the meta block. | γ |
| B4 | purpose-body ERROR | `decl_passes_purpose_body_sub_swallowed.ri` | rc 1; a parse error at the `sub` line. | γ |
| B5 | reflective parity | `crates/reify-cli/tests/fixtures/stdlib_sim_ready_{geom_undef,material_active,material_ok}.ri`, `examples/determinacy_intrinsics.ri`, `examples/m5_purpose.ri` | `reify check` stdout+stderr byte-identical to the `8e6ffcac9d` capture (manifest δ). | δ |
| B6 | reflective hash stability | a purpose using each kind | `PurposeReflectiveAggregation` content hash equals the hash computed from the literal string. | δ |
| B7 | deferred connect with a literal index | a `forall v in vents: connect vents[0].<p> -> v.<q>` body (direction-compatible ports; shape after `forall_runtime_re_elaboration.rs`'s Connect fixture) with `n` undef, then `Engine::edit_param(n = 3)` | Connections equal the resolved-path connections for the same source compiled with `n = 3`: `vents[0].<p> → vents[i].<q>` for `i` in 0..3. RED on main per `rewrite_port_placeholder`'s own doc (read, not probed: the deferred path is reachable only through `edit_param`). | ε |
| B8 | connector parity | `examples/auto_binding_sites.ri`; reify-eval connector suites | `reify check` output byte-identical (4 indeterminate today); suites green; R2 = 1. | ζ |
| B9 | user-named `__connector_0` | `sub __connector_0 = Gain()` | Its `SubComponentDecl.origin == Declared`. | ζ |
| B9b | **[Amended 2026-10-20, retro-critic R4]** user `__connector_0` beside a connector connect | `Top { sub __connector_0 = Gain()  port a: out Sig {}  port b: in Sig {}  connect a -> b : Conn { gain = auto } }`, Conn pinning `gain == 10mm`, Gain.gain = 3mm (RED on main: `Top.__connector_0.gain = 0.003 m`, rc 0, no diagnostic) | rc 1 with the reserved-prefix Error, or (mint-skips variant) the connector keeps 10mm. | ζ |
| B10 | phantom-default cases via source | each of the four same-name-loser shapes behind the skip sets, as `.ri` through `check_trait_conformance` | Same advertised and injected sets as main; no `check_phase_*` call in the test region. | η (pins), θ (stays green) |
| B11 | acceptance sites agree | **[Amended 2026-10-29, esc-8739-2]** (ι) each deliberate divergence row, at a struct-ctor site and at a fn-call site whose parameter carries a trait object; (#9571) the same rows at a fn-call site with a concrete parameter and at a param-default site | (ι) Main's exact verdict and diagnostic, reached through `conformance_accepts`; the walker has no type-comparison arm. (#9571) The ruled verdict per row at all three sites, as listed in #9571's verdict table. | ι (narrowed), #9571 (full) |
| B12 | DOF gates | a joint with a `Type::Error` DOF; a joint with a non-kind DOF type; a joint whose body has `tangent` | No `E_JOINT_DOF_MISMATCH`; the non-kind case gets exactly one `E_ARG_TYPE_MISMATCH`. | κ |
| B13 | relation table | iterate `RelationKind::iter()` through `relation_sig` | **[Amended 2026-10-20, retro-critic R6]** The names derive from the reify-builtins rows; arity is the row's; the doc count matches; R3 = 0. (The rot + trans == ΔDOF clause moved to #6669 B4 — lead amendment, D9.) | λ |
| B14 | parse-error kind | a member-continuation source through `compile_project` (module_dag) | Diagnostic carries the mapped code and the original span; CLI `render_errors` text unchanged. | μ |
| B15 | value-cell union | a template with guarded `where`/`else` members | `all_value_cells()` yields value_cells, then each group's members, then else_members. | ν |

## §7 Cross-PRD relationship (G4)

| other PRD / stream | direction | seam mechanism | owner | status |
|---|---|---|---|---|
| S27 tooling front end | produces → | `TopologyTemplate::all_value_cells()` | **S9** (program G4 table) | ν |
| S27 tooling front end | produces → | `ParseErrorKind::diagnostic_code()` for `compile_user_module` and LSP `convert_parse_error` | S9 produces; S27 adopts | μ |
| reify-ast `MemberDecl` variant additions and their dispatch sites | owns | `member_support` table | **S9** (program G4 table) | γ |
| S7 compiler module namespace (`docs/prds/v0_6/compiler-module-namespace.md`) | consumes ← | S7 β (`member_cell_decls.rs`; guards.rs explicit imports), S7 θ (entity.rs explicit imports), S7 ζ (conformance/ explicit imports); lib.rs `mod` lines for `let_kind`, `member_support`, `type_acceptance` follow S7 D3 (top-level siblings importing downward) | S7 | ξ depends on S7 β and θ; ο on S7 ζ; every S9 leaf lands after S7 per the program's compiler order. **[Amended 2026-10-20, retro-critic R4]** Edges: β #8731 ← #8615, #8627; γ #8732 ← #8615, #8627; δ #8733 ← #8615 (traits.rs) |
| S6 kinematics | consumes ← | **[Amended 2026-10-20, retro-critic R6]** S6 γ #8830 binds `reify_builtins::RelationKind` (exhaustive `describe_demand` / `residual_dispatch`; reify-constraints and reify-eval depend on reify-builtins); ~~`RelationKind` in reify-ir~~ (superseded by R6) | S6 | λ depends on #8830 (released 2026-10-13) |
| S8 builtin registry | consumes ← | **[Amended 2026-10-20, retro-critic R6]** `reify_builtins::RelationKind` + the relation rows (S8-ψ #9418); ~~produces → `RelationSig` relation facts that reify-builtins rows read~~ (superseded by R6) | S8 | λ after S8-ψ (`#8741 ← #9418`) |
| geometry-algebra-solver-unification (#6669 `LoweringTable`) | produces → | **[Amended 2026-10-20, retro-critic R4]** λ's arity-from-row form of `relation_delta_dof(_kinds)`; #6669 keys on `reify_builtins::RelationKind` (R6) | that PRD | `#6669 ← #8741`; #6669 keeps or lowers R3 in the same diff |
| S4 cell-eval closure | produces → | engine_edit.rs per-element connect materialisation via `PortPath::at_element`; engine_eval.rs connector recognisers on `SubOrigin` | S9 edits first; S4 partitions those files in wave 2 | ε, ζ |
| uniform-member-access (`docs/prds/v0_6/uniform-member-access.md`) | consumes ← | #5425 post-acceptance `is_geometry_let`; #5430 owns member_path consumption; **[Amended 2026-10-20, retro-critic R4]** produces → `MemberKinds` for #5426 (`TypedGeometryAlias` refinement step) and #5428 | that PRD | β depends on #5425; #5426 ← β, #5428 ← β |
| stdlib-namespace (`docs/prds/v0_6/stdlib-namespace.md`) | consumes ← | #5494 deletes the skeleton (β-compatible); #5499 owns stdlib import edges | that PRD | reference; **[Amended 2026-10-20, retro-critic R4]** #5494 lowers R1 in the same diff |
| specialization-body tasks #8071, #8195, #8202, #8203 | produces → | **[Amended 2026-10-20, retro-critic R4]** `member_support` `SpecializationBody` rows + `unsupported_member_diagnostic` | S9 (table) | #8071, #8195, #8203 ← γ #8732; #8202 consumes the row |
| S12 check surface | produces → | S12 lists S9 upstream; S9's contracts are §5 | S9 | the lead names the S12-required contract if it is outside §5 |

## §8 Decomposition plan

Each leaf carries `metadata.x_finding_key` (list), `x_finding_run = "review-all-reify-20261010"`,
`source = "review-all"` and priority = severity. A task saying "extract X" names its ratchet row.
New gate-resident tests carry their drift-guard registrations in the same diff (overlay
"Gate-test drift-guard registration").

- **α #8730 — Construct-ratchet member for the declaration passes** *(intermediate → β, ζ, λ, ν)*.
  Modules: `tests/infra/test_compiler_decl_pass_ratchets.{py,sh}`,
  `tests/infra/run-all-classification.manifest`. Ships R1–R3 at today's baselines (17 / 4 / 10),
  with hermetic flag/clear fixtures; **[Amended 2026-10-20, retro-critic R4]** rows are two-sided (a below-baseline count is STALE)
  and R1 counts by predicate name list (D12). Signal: the member runs under `run_all.sh`, its fixture cases
  red on an over-baseline file and green on the clean twin, and it is green on main. Priority
  medium. Keys: none (infrastructure for INV candidate 5).
- **β #8731 — Classify each let once: `LetKind` / `MemberKinds`** *(leaf)*. Modules: reify-compiler
  `let_kind.rs` (new), entity.rs, guards.rs, geometry_list.rs as needed. D1. Signal: B1, B1b; R1 at
  1; `compile_entity`'s four prologue sets gone (the check goes green only once β also converts
  `build_structure_def_skeleton` or #5494 has deleted it); the function doc no longer says
  "Two-pass". **[Amended 2026-10-20, retro-critic R4]** Classifier takes the resolved param types; named refinement steps (D1).
  Deps: α, #5425, S7 β #8615, S7 θ #8627. Dependents: #5426, #5428. Priority **high**. Keys: fk-0bdf9598b402.
- **γ #8732 — One member-support table; no silent member drops; purpose-body parse errors reported**
  *(leaf)*. Modules: reify-compiler `member_support.rs` (new), entity.rs, guards.rs, traits.rs,
  compile_builder/sketch_unsupported.rs; reify-syntax ts_parser.rs (`lower_purpose_members`);
  reify-ast decl.rs (`MemberDecl::tag`). D2. Signal: B2, B3, B4; the 11 duplicate strings and
  `unsupported_purpose_member_info` gone. G7: establishes INV-SF-3 for member positions. Priority
  medium. **[Amended 2026-10-20, retro-critic R4]** `MemberContext` includes `SpecializationBody` and `MatchArm` (D2). Deps: S7 β #8615,
  S7 θ #8627. Dependents: #8071, #8195, #8203 (table rows); #8202 consumes the `SpecializationBody`
  row. Keys: fk-196deab564e7.
- **δ #8733 — `ReflectiveQueryKind` end to end** *(leaf)*. Modules: reify-ir expr.rs; reify-compiler
  types.rs, traits.rs, expr.rs; reify-eval engine_purposes.rs; reify-test-support. D3. Signal: B5,
  B6; no `query_kind: String`. **[Amended 2026-10-20, retro-critic R4]** Deps: S7 β #8615 (traits.rs). Priority medium. Keys: fk-30505ec518b4, fk-ee1be91862ca.
- **ε #8734 — Typed `PortPath` for deferred forall-connect endpoints** *(leaf)*. Modules: reify-compiler
  types.rs, forall_elaborate.rs, connect.rs (`resolve_port_name` callers); reify-eval engine_edit.rs.
  D4. Signal: B7 (RED first on main: the literal endpoint is rewritten); `rewrite_port_placeholder`
  deleted; no Info string cites a task number; the module doc states that Connect is materialised
  at runtime. Priority medium. Keys: fk-27489259525d.
- **ζ #8735 — `SubOrigin` on `SubComponentDecl`** *(leaf)*. Modules: reify-compiler types.rs, connect.rs
  and the `SubComponentDecl` construction sites; reify-eval engine_eval.rs, resolve_order.rs;
  reify-test-support builders. D5. Signal: B8, B9, **[Amended 2026-10-20, retro-critic R4]** B9b (reserved-prefix Error); R2 at 1; no
  `starts_with("__connector_")` in crates/reify-eval/src. Deps: α. Priority medium. Keys:
  fk-76bdd62534fe.
- **η #8736 — Conformance tests through the conformance entry, not the phases** *(leaf)*. Modules:
  reify-compiler conformance/mod.rs tests, plus a `.ri` fixture home per overlay tier
  (`crates/reify-compiler/tests/fixtures/`, or inline sources via the existing `run_conformance`
  harness). Signal: zero `check_phase_` calls in the test region (19 today); the tests that pin the
  parser-unreachable `RequirementKind::Let` are deleted, or their state is made unrepresentable; the
  module doc is replaced by a one-line pointer to this PRD; B10 cases green on main before θ.
  Priority medium. Keys: fk-254bc351a8ab.
- **θ #8738 — `DefaultDisposition` replaces the four skip sets** *(leaf)*. Modules: conformance/checker.rs,
  conformance/mod.rs. D6. Signal: B10 green unchanged; the four `&HashSet<String>` parameters are
  gone from both phases; the prose table is deleted. Deps: η. Priority medium. Keys: fk-cf6356374fa1.
- **ι #8739 — `conformance_accepts` beside `type_compatible`** *(leaf)*. Modules: reify-compiler
  `type_acceptance.rs` (new), conformance/mod.rs. D7. Signal: B11 (ι half, **[Amended 2026-10-29,
  esc-8739-2]** behaviour-preserving; the three-site half is #9571); the walker's own-rule arms
  delegate; its divergence prose is deleted, with a one-line pointer per named arm; **[Amended 2026-10-20, retro-critic R4]** the
  outcome type is `ArgVerdict` and takes the registries; the walker keeps the TraitObject recursion.
  Deps: η.
  Priority medium. Keys: fk-37775e022693.
- **κ #8740 — One gated DOF entry point** *(leaf)*. Modules: joint_self_check.rs,
  compile_builder/entities_phase.rs. D8. Signal: B12; one `pub(crate)` item in the module; the
  entities_phase §(4) block is one call. Priority medium. Keys: fk-cc676a29f5d9.
- **λ #8741 — `RelationSig` table keyed by `RelationKind`** *(leaf; was HELD on S6; released 2026-10-13 by S6's session with dependency S6 γ #8830 — RelationKind parsed at reify-eval relate_solve::relation_instance (S6 R4); ~~λ keys on `RelationKind::from_name`~~ (superseded by R6: `from_name` never exists; `lookup(name, argc)` is the one parser))*. **[Amended 2026-10-20, retro-critic R6]** λ's table is the total
  `fn relation_sig(RelationKind) -> RelationSig { operands, metric_slot }` over reify-builtins'
  generated `RelationKind`; arity and result type are read from the row; `RELATION_FN_NAMES` goes.
  **[Amended 2026-10-20, retro-critic R4]** It replaces the arity guards inside `relation_delta_dof(_kinds)` with the row arity, values
  unchanged. Modules: relation_signatures.rs (and
  its LSP callers' imports, if signatures move). D9. Signal: B13; R3 at 0; the tests iterate
  `RelationKind::iter()`. Deps: α, κ, S6 γ #8830, S8-ψ #9418. Dependent: #6669. Priority medium. Keys:
  fk-cb941b3d058e.
- **μ #8742 — `ParseErrorKind` on `ParseError`** *(leaf)*. Modules: reify-ast decl.rs; reify-syntax
  ts_parser.rs, member_continuation.rs; reify-compiler module_dag.rs; reify-core diagnostics.rs (new
  codes); the four consumer test files. D10. Signal: B14; `is_member_continuation_message` gone.
  Priority low. Keys: fk-ddfdce77efbc.
- **ν #8743 — `TopologyTemplate::all_value_cells()` (seam for S27)** *(leaf)*. Modules: reify-compiler
  types.rs plus the reify-compiler chain sites; α's member (adds R4). D11. Signal: B15; R4 baseline
  committed, with reify-compiler at 0. Deps: α. Priority medium. Keys: none of S9's (the G4 seam for
  S27's fk-b420a7695f68).
- **ξ #8744 — h14 partition of entity.rs** *(leaf)*. Modules: entity.rs plus new sibling files per the
  §2.6 row (AST walkers; the cell and bound helpers that remain after S7 β moves the five
  guards.rs-facing cell builders to `member_cell_decls.rs`; objective and annotation checks;
  match-arm grouping once S7 θ removes the `use super::*` reach-back; tests out of line). The
  skeleton group is not moved (#5494 deletes it). `compile_entity` stays whole as a file move; its h4 cost is out of scope (§10).
  Signal: each new file passes h13 (no upward import, no reach-back) and entity.rs is below the
  2,000-line alarm or the commit message records why the residual stays; the commit message carries
  the measurement. Deps: β, γ, S7 β #8615 (`member_cell_decls.rs`), S7 θ #8627 (entity.rs explicit imports). Priority medium.
  Keys: none (h14 measurement).
- **ο #8745 — h14 partition of conformance/mod.rs** *(leaf)*. Modules: conformance/ (ctx module for
  `WalkCtx`/`diag_at`, walker, geometry leaf, arg conformance, joint bound; tests out of line).
  Signal: as ξ; the walker ↔ geometry-leaf cycle is broken by the downward ctx module. Deps: η, θ,
  ι, S7 ζ #8623 (conformance/ explicit imports, including the sibling `use checker::*;`). Priority medium. Keys: none (h14 measurement).
- **π #8746 — PRD close** *(leaf; overlay decompose-close obligation)*. Deps: every leaf above, plus #5499 and #5430 (lead ruling: S9 cannot close before the two adopted owners land). Signal:
  the committed SHIPPED header with landed leaf ids, the AS-AUTHORED freeze paragraph and the
  LIVE/AS-AUTHORED map, applied to the `.capability-manifest.md` too. Priority low.

Counts: 16 leaves; high 1 (β), medium 13 (α γ δ ε ζ η θ ι κ λ ν ξ ο), low 2 (μ π).

## §9 Finding-key disposition

| key | leaf / disposition |
|---|---|
| fk-0bdf9598b402 | β |
| fk-196deab564e7 | γ |
| fk-30505ec518b4 | δ |
| fk-ee1be91862ca | δ |
| fk-27489259525d | ε |
| fk-76bdd62534fe | ζ |
| fk-254bc351a8ab | η |
| fk-cf6356374fa1 | θ |
| fk-37775e022693 | ι |
| fk-cc676a29f5d9 | κ |
| fk-cb941b3d058e | λ |
| fk-ddfdce77efbc | μ |
| fk-60053402aba5 | not addressed here because #5499 (stdlib-namespace ζ) owns it: declared `import` headers for every stdlib module, order derived, ordering comments deleted, identity test replaced by a determinism test. Disposition `filed:5499`: key stamped on #5499 at decompose; π #8746 depends on #5499. |
| fk-6621fbaa0058 | not addressed here because #5425 lands `resolve_member_path`'s first production consumer and #5430 retires the expr.rs parallel authorities. Disposition `filed:5430`: key stamped on #5430 at decompose; π #8746 depends on #5430; #5425 is a hard dependency of β #8731. |
| fk-75d9313c6ee6 | not addressed here because it is mechanical and already filed as #8556; no leaf subsumes it. Disposition `filed:8556`. |

## §10 Out of scope

- Splitting `compile_entity` into per-pass functions (h4). β makes the shared state one value, which
  is the precondition. The split is a follow-up for whoever next changes a pass.
- Typing `CompiledConnection.left_port`/`right_port` and instance paths (`ValueCellId.entity`):
  S3's `normalize_cell_id` finding.
- The forall **Constraint** body's `<parent>.<sub>[0]` cell-id rewrite, which has the same
  user-literal divergence (types.rs `CompiledForallBody::Constraint` doc). It is not in a finding
  key. Noted for a follow-up.
- The unannotated-let forward-reference limitation in trait defaults (`PreRegisterOutput` DESIGN
  LIMITATION).
- Adding a fourth reflective query kind (`ports`).
- Stdlib ordering (#5499), the skeleton mirror (#5494), member-path consumption (#5425/#5430).

## §11 Open questions (tactical)

1. **`MemberKinds` key.** Name, or declaration occurrence (guarded `where`/`else` arms can reuse a
   name)? Suggested: occurrence index into the walked member list, with a name lookup for readers
   that only hold a name. Decide in β.
2. **Parse-error code spellings.** `E_PARSE_SYNTAX` / `E_MEMBER_CONTINUATION` are working names.
   Decide in μ, and add the codes' doc lines without growing the per-variant prose that S23 is
   burning down.
3. **η fixture home.** Inline sources through `run_conformance` versus files under
   `crates/reify-compiler/tests/fixtures/`. Either keeps the overlay's fixture tiers. Decide in η.
4. **ParseErrorKind breadth.** Two kinds suffice for the finding. Add further kinds only where a
   consumer distinguishes them today. Decide in μ.

## Gap-fill amendments (2026-10-16)

Rulings from the attended gap-fill sitting, `plans/gap-fill-deliberation-reify-20261016.md`. They add leaves and amend one; the sections above are unchanged except the status line.

- **ρ #9321 — reify-ast child enumeration** (§K2-D2; high, gate #9171; fk-8d86383441a3, plus the derivation-walk half of fk-406a3917737c). `Expr::for_each_child` (+ `_mut`), `MemberDecl::for_each_direct_expr` (including derivation operands, override values/guards, `using_plane`) and `MemberDecl::child_scopes` (including `derivation.members`) in reify-ast; `walk_members` re-expressed over `child_scopes`; an adoption-ratchet row counting hand-rolled `ExprKind`/`MemberDecl` child matches outside reify-ast in α #8730's member (ρ depends on #8730). Consumers: S27 ψ₂ #9324 and ψ₃ #9325 (`docs/prds/v0_6/tooling-front-end.md`), #7860 (moved to its option (b) and extended to delete the shadow_lint / dot_chain_lint / entity.rs copies; depends on ρ). `child_scopes` is member-level and disjoint from S27 η #8772's declaration-level `Declaration::member_scopes`. New G4 row (program doc): `reify-ast Expr/MemberDecl child enumeration | S9 ρ | S27, #7860, compiler lints`.
- **σ #9322 — `SpannedIdent` for declared and referenced names** (§K2-D2; high, gate #9171; fk-c008cdb5b456). Field by field: `structure_name`, `type_name`, `ConstraintInst` name first, then declaration names, `FunctionCall` callee and `TypeParamDecl.bounds`; reify-lsp's `name_token_span` and the three empty-span conventions deleted. The spans are populated at the lowering site by the syntax-lowering-contract stream (K2-D1, not yet filed; its /prd session wires σ ← its lowering leaf). #7541, #7683, #7684, #8610 and #7549 are re-pointed onto σ (each depends on it). New G4 row: `SpannedIdent on declared/referenced names | S9 σ (fields) + syntax-lowering-contract (lowering) | reify-lsp decl_oracle, #7541/#7683/#7684/#8610`.
- **ε₂ #9323 — `compile_connection` keeps the typed `PortPath`** (§Uncontested block K9-D1; medium; fk-98a6b0d91a7f, review-reify-20261015). Under D4 beside ε #8734: own vs sub-member endpoint comes from `PortRoot`, `Named("self")` folds into the own-port case at parse, direction and member lookup are keyed by (root sub, members), and the String is rendered only where `CompiledConnection` and labels need it; `own_port_name`'s and `sub_member_endpoint`'s string parsing deleted. #7659 is subsumed (its fixture and per-element hint ported into ε₂; cancelled). #7162 (assembly-modal α) and #7881 depend on ε₂; `docs/prds/v0_6/assembly-modal-connection-graph.md` now says α extends `PortPath` members rather than `resolve_port_name`. Boundary rows: `connect self.p -> sub.q` and `connect p -> sub.q` classify identically; `connect vents.outlet -> hub.feed` with `sub vents : List<Vent>` is diagnosed with the per-element form suggested; a nested `a.b.p` endpoint naming an undeclared sub is an undefined-port error, never Satisfied. *(Ruling 2026-10-18, Leo, esc-9323-3 option A: this row is pinned in two parts. ε₂ delivers the depth-1 typed undefined-port error (`ghost.outlet`, an undeclared sub). At depth ≥2 the endpoint stays a parse-level Error ("invalid port reference", never Satisfied), and the typed undefined-port message is α #7162's acceptance, since α owns nested `PortPath` members.)*
- **γ #8732 amended** (§K2-D1). The program's pass-1 Q5 premise ("no other stream owns `ts_parser::lower_purpose_members`") no longer holds: the new syntax-lowering-contract stream owns reify-syntax's lowering dispatch, body loops and fault policy. γ's parser-side hunk (`lower_purpose_members` reporting an ERROR child) is ceded to that stream's member-list leaf, or lands first as an interim fix the stream subsumes. γ's compiler-side member-support table, §5 rows, B2–B4 and delivered checks are unchanged.
- **Close leaf.** π #8746 depends on every leaf above in §8; adding ρ, σ and ε₂ to π's dependencies is left to the S9 lead (this sitting's applier did not write #8746).

## Retro-critic sitting amendments (2026-10-20)

Rulings from the attended retro-critic sitting on the stream's adversarial critique
(`~/.claude/review-all/review-all-reify-20261010/retro-critic/S9.json`). The sections above are
amended in place where marked `[Amended 2026-10-20, retro-critic R<n>]`.

- **B1 (λ #8741 vs S8 #6944) — relation-signature facts have one home, the builtin registry.**
  Supersedes program pass-1 ruling S9 Q3(a) ("S6 mints RelationKind; S9 λ owns the RelationSig
  rows; S8 reads them"), D9 as authored, §5 row 10 and §7's S8 row. `RelationKind` is reify-builtins'
  generated `Relation` sub-enum minted by the new S8 leaf **S8-ψ #9418**; λ's table is a total
  `fn relation_sig(RelationKind) -> { operands, metric_slot }` reading arity and result type from
  the row; no `RELATION_FN_NAMES`, no `from_name`. Edges: `#8741 ← #9418`, `#8830 ← #9418`,
  `#6944 ← #9418`; `#6944 ← #8741` removed. #8830 (S6 γ) binds the enum with reify-constraints and
  reify-eval depending on reify-builtins; reify-ir's relation_kind.rs is not created. λ's delivered
  checks gain `relation-names-derive-from-rows` and `relation-sig-reads-the-registry`. Pointer:
  `plans/review-all-program-reify-20261012.md §Resolved decisions — retro-critic sitting R6`.
- **B2 (γ #8732) — γ first.** `MemberContext` gains `SpecializationBody` and `MatchArm`; both join
  B2's matrix; §5's invariant covers every MemberDecl loop. Edges `#8195, #8203, #8071 ← #8732`;
  those three add `member_support` rows plus `unsupported_member_diagnostic` (no per-kind module, no
  `E_SPEC_BODY_*` codes unless the table carries them); #8202 consumes the `SpecializationBody` row;
  §3 lists all four. Pointer: `plans/review-all-program-reify-20261012.md §Resolved decisions — retro-critic sitting R4`.
- **B3 (β #8731) — β first.** Edges `#5426 ← #8731`, `#5428 ← #8731`; #5426 adds the `MemberKinds`
  variant `TypedGeometryAlias`, written by a named refinement step after pass-2 typing, and carries
  files entity.rs, geometry.rs. D1 and §5 row 1 name every refinement step and its input; R1 counts
  let-kind predicates by name list; §3/§7 list #5426 and #5428. Pointer: `plans/review-all-program-reify-20261012.md §Resolved decisions — retro-critic sitting R4`.
- **M1 (ι #8739).** `conformance_accepts` takes the registries; the outcome is `ArgVerdict` with
  `QuantityConflict{param_q, arg_q}` and `RequiresTraitConformance(trait)`; the walker keeps the
  TraitObject recursion (D7, §5). Pointer: `plans/review-all-program-reify-20261012.md §Resolved decisions — retro-critic sitting R4`.
- **M2 (ζ #8735).** Reserved-prefix Error for a user sub or member named `__connector_*` (or a mint
  that skips taken names); boundary row B9b with the collision fixture; a test-builder exception to
  "only connect.rs constructs Connector" (D5, §5, §6). ζ's `recognisers-gone` check widened to
  `crates/reify-eval/src`. Pointer: `plans/review-all-program-reify-20261012.md §Resolved decisions — retro-critic sitting R4`.
- **M3 (S7 order).** Edges `#8731 ← #8615, #8627`; `#8732 ← #8615, #8627`; `#8733 ← #8615` (§7, §8);
  the waiver option was declined. Pointer: `plans/review-all-program-reify-20261012.md §Resolved decisions — retro-critic sitting R4`.
- **M4 (λ / #6669).** λ replaces the arity guards inside `relation_delta_dof(_kinds)` with the row
  arity, values unchanged (supersedes the 2026-10-13 codim amendment's "λ leaves
  relation_delta_dof(_kinds) computing as today" clause); edge `#6669 ← #8741`; #6669 carries the
  note to keep or lower R3 in the same diff (D9, D12, §3). Pointer: `plans/review-all-program-reify-20261012.md §Resolved decisions — retro-critic sitting R4`.
- **M5 (α #8730).** D12's rows are two-sided (a below-baseline count is STALE and the row is lowered
  in the same diff); #5494 and #6669 carry same-diff "lower the row" notes and §3 lists them; the
  ceiling-only option was declined. Pointer: `plans/review-all-program-reify-20261012.md §Resolved decisions — retro-critic sitting R4`.
- **M6 (β #8731).** The classifier takes the resolved param types (computed first) and returns
  `GeometryList(shape)` only after a pure element-cap check; pass 1 emits the list diagnostics from
  the verdict; §5 row 1 states the order. Pointer: `plans/review-all-program-reify-20261012.md §Resolved decisions — retro-critic sitting R4`.
- **Polarity and drift (twins).** κ's single-entry check also matches a comment in entities_phase.rs
  (κ's record says to delete or reword it in the same diff); β's prologue-sets-gone check goes green
  only once β converts the skeleton or #5494 has deleted it; σ #9322's OPEN `lowering-populates-spans`
  row is bound to σ itself (syntax-lowering-contract R4: σ owns `SpannedIdent` population at the
  ts_parser.rs lowering sites); D11/D12's R4 ownership corrected (reify-eval and reify-test-support
  rows are S4's, #8870; only reify-lsp's are S27's). Pointer: `plans/review-all-program-reify-20261012.md §Resolved decisions — retro-critic sitting R4`.

## esc-8739-2 amendment (2026-10-29)

Ruled by Leo. **B11's premise was false; ι is narrowed.** At main `129a711d2a`, for
a concrete parameter, fn-call rejects what ctor and default accept silently (`Vector3<Length>` ←
`vec3(1,2,3)`, `Point3<Length>` ← `5`, `Field<Point3<Length>,Real>` ← wrong-domain lambda,
`Matrix<3,3,Real>` ← list), because three relations decide acceptance (overload slot matching, the
walker, the default check). A SPOT violation (h11). Ruling: ι stays the behaviour-preserving
extraction and carries the ctor/trait-carrying half of B11. The structural task #9571 (deps ι,
γ₂ #9363, #8121, #8317, #8101, #9568) routes fn-call concrete slots and param-default cells
through `ArgVerdict` and carries full B11. Per-row verdicts: Point/Matrix-scalar/Geometry reject
everywhere (#8121, #8317 — Matrix verdict pending Leo's explicit confirmation —, #8101, re-pointed to edit the relation's arms); Vector ← dimensionless
rejects everywhere (V2, #9568, held on the direction-type ruling); Field ← lambda and Matrix ←
nested list are Unverifiable everywhere until #9569 / #9570 remove the erasure. D7 and B11 are
amended in place.
