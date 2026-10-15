# PRD: Compiler declaration passes — one classification per member, one acceptance relation

**Status:** active — authored 2026-10-12 (pass 1 of the `/prd` program for stream **S9** of
`plans/review-all-program-reify-20261012.md`); decomposed 2026-10-12 (pass 2): α #8730 … π #8746,
λ #8741 held on S6. Shape: **B+H** (contract §5,
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
| #5494 stdlib-namespace γ (authoritative templates before fn bodies; deletes `build_structure_def_skeleton`) | pending | β is order-independent of it (§4 D1). ξ does not move the skeleton group. |
| #5499 stdlib-namespace ζ (declared `import` headers for all stdlib modules; strict per-module compile; ordering comments deleted; NS-M3 determinism test) | pending; deps #5494, #5496 (done), #5497, #5498 | **Owns fk-60053402aba5** (§9). It is strictly stronger than the review's remedy. |
| #8556 (mechanical: correct `ambient_defaults.rs` module doc) | pending | **Owns fk-75d9313c6ee6** (§9). Not subsumed by any leaf here. |
| S7 (explicit `use crate::…` imports replace the crate-root globs and `use super::*`) | wave 1, earlier in the compiler order | ξ and ο depend on its leaves for entity.rs and conformance (§7). |
| S6 (`RelationKind` enum in reify-ir, parsed once by the compiler) | wave 1, spawn | λ depends on its RelationKind leaf (§7). |

No novel grammar. Every fixture in this PRD parses with the current tree-sitter grammar, except
`decl_passes_purpose_body_sub_swallowed.ri`, whose grammar rejection is the point (§2.2).
`grammar_confirmed = true` for every leaf.

## §4 Resolved design decisions

**D1. LetKind is computed once, by a stratified, order-free classifier (β).** A new module
`let_kind.rs` exposes:

```
classify_members(members: &[MemberDecl], functions: &[CompiledFunction]) -> MemberKinds
```

It computes selectors to a fixpoint first, because `is_selector_expr` depends only on the selector
set. It then computes geometry to a fixpoint given the final selector set, because `is_geometry_let`
is monotone in the geometry set once the selector set is fixed. It then classifies geometry lists
and rejected lists given both sets. Everything else is `Value`. A forward alias is therefore
classified the same as a backward one. A cycle (`let a = b; let b = a`) is the least fixpoint, so
neither let is geometry, as `cyclic_ident_alias_does_not_crash` pins today.

`MemberKinds` maps every declared let and geometry-defaulted param occurrence to a `LetKind`, and
covers members inside guarded `where`/`else` arms. The variants are Geometry, Selector,
GeometryList(shape), RejectedGeometryList, CrossSubGeometryAlias and Value.

The one scope-dependent classification, `is_bare_cross_sub_geometry_alias`, needs the sub types
that pass 1 registers. It becomes a refinement step, run once immediately after pass-1 sub
registration, that writes `CrossSubGeometryAlias` into the same table. Pass 2 treats that variant
as a value cell and pass 3 treats it as a realization, which is exactly today's split, now named.
Every reader after the classifier reads `MemberKinds`: pass 1's scope registration, pass 2, pass 3,
`collect_geometry_exprs`, `register_guarded_names`, `compile_guarded_members`, and
`build_structure_def_skeleton` if it still exists when β lands.

β therefore does not wait for #5494. The skeleton calls the same function, and #5494 deletes that
call later. The pre-classification hoist predicate `is_hoistable_query_call` keeps its direct
syntactic call, because it runs before any member is classified. It is the one named exception in
ratchet R1.

**D2. One member-support table, no silent disposition (γ).** A new module `member_support.rs`:

```
enum MemberContext { EntityBody, PortBody, GuardedBlock, TraitBody, PurposeBody, PurposeGuardedBlock }
enum MemberSupport { Compiled, HandledBy(Owner), Rejected }
fn member_support(kind: MemberKindTag, ctx: MemberContext) -> MemberSupport
fn unsupported_member_diagnostic(kind: MemberKindTag, ctx: MemberContext, span: SourceSpan) -> Diagnostic
```

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

**D6. `DefaultDisposition` replaces the four skip sets (θ).** `check_phase_pre_register_default_types`
returns one `DefaultDisposition` per `(name, AvailableDefaultKind)`. The variants are Inject,
SlotTakenByAnnotatedLet, SlotTakenByParam, SlotTakenByUnannotatedLet and CompileError (working
names). `PreRegisterOutput::disposition(name, kind)` is the single accessor. Both phases match on it
exhaustively, and the `cell_type` conjunctions disappear because the kind is in the key. The
set-to-guard prose table is deleted. The forward-reference limitation of unannotated lets is
unchanged (§10).

**D7. `conformance_accepts` is one named relation beside `type_compatible` (ι).** A new module
`type_acceptance.rs` defines
`conformance_accepts(param: &Type, arg: &Type) -> Acceptance { Accept, Reject, Unverifiable }` and
imports `type_compat::type_compatible` downward. It does not live in type_compat.rs: that file is
5,299 lines, and S7/S8 partition it. Each deliberate divergence from `type_compatible` is an
explicitly named arm with a one-line pointer to its rationale in the language spec or this PRD. The
Vector-accepts-any-`Tensor<1>` rule is one example. The walker keeps only wrapper recursion and
diagnostic emission. Fn-call, struct-ctor and default sites all reach acceptance through the
relation, so they cannot disagree.

**D8. One gated DOF entry point (κ).**
`joint_dof_diagnostics(joint_name, dof: &[(name, Type, span)], body: &[CompiledExpr], span) -> Vec<Diagnostic>`
owns both gradualism gates and the verdict. The pieces become private, and tests drive the entry
point.

**D9. One `RelationSig` row per relation kind (λ).** A static table holds
`{ kind: RelationKind, arity: Arity, operands: OperandPolicy, metric_slot }`. It is keyed by S6's
`RelationKind`. `RELATION_FN_NAMES`, `is_relation_typed_fn`, `relation_metric_slot`,
`relation_operand_datum` and the arity-3 policy all derive from it, and the tests iterate it.

*Lead amendment 2026-10-13 (codim seam, raised by S6's session):* the codimension has ONE home,
geometry-algebra-solver-unification β #6669's `LoweringTable` (`Row.codim`, keyed by
(RelationKind, DatumKind, DatumKind)), which that earlier PRD already owns together with the measured
`individual_rank` half. `RelationSig` therefore carries no `codim` field. λ leaves
`relation_delta_dof(_kinds)` computing as they do today, keyed on `RelationKind`; #6669 derives them
from `Row.codim`. B13's rot + trans == ΔDOF clause moves to #6669's B4 ("the compiler's published
codim and the solver's codim are the SAME FIELD").

Builtin result typing and arity are also S8's reify-builtins row facts. λ keeps them in the row only
as relation semantics ("which overload is a relation"), and S8's rows read `RelationSig` rather than
re-declaring it (cross-reference §7). `units.rs::datum_constructor_result_type`'s arity-2 `offset`
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
reify-compiler chains through it. S27 routes reify-lsp, eval_guard and the remaining workspace
chains. Ratchet R4 counts what is left.

**D12. One construct-ratchet member (α).** `tests/infra/test_compiler_decl_pass_ratchets.py` (stdlib
`unittest`) has a thin `test_compiler_decl_pass_ratchets.sh` wrapper and a
`run-all-classification.manifest` row in the same diff. It counts code constructs (comment lines
excluded) against committed baselines, and hermetic fixtures prove that each count both flags and
clears. That makes it a construct lint, not a prose pin (tests/infra/README.md "Assert behaviour,
not source prose"). The member asserts no wall-clock bound.

| id | construct | scope | baseline at `8e6ffcac9d` | target | lowered by |
|---|---|---|---|---|---|
| R1 | call of `is_geometry_let(` / `classify_geometry_list_let(` / `is_selector_expr(` | entity.rs, guards.rs | 17 | 1 (`is_hoistable_query_call`) | β |
| R2 | `"__connector_` string literal | `crates/*/src` production code (excluding `#[cfg(test)]` modules) | 4 | 1 (the const) | ζ |
| R3 | `args.len() == 3` | relation_signatures.rs production region | 10 | 0 | λ |
| R4 | inline `guarded_groups` → `members`/`else_members` value-cell union outside `all_value_cells` | `crates/*/src` production code | measured by ν | reify-compiler 0; the rest S27 | ν, then S27 |

Members of extraction leaves name their ratchet row (INV candidate 5, extract-with-ratchet).
Leaves whose superseded pattern becomes unrepresentable by type need no row: δ (String gone),
ε (template strings gone), θ (sets gone), μ (substring predicate gone).

**D13. Already-owned remedies are not re-filed.** fk-60053402aba5 → #5499, fk-6621fbaa0058 → #5430
(with #5425), and fk-75d9313c6ee6 → #8556 (§9). Filing duplicates would fight those tasks for the
same files, which contract §8 step 2 exists to prevent.

## §5 Contract (B+H)

| seam | producer | consumers | invariant |
|---|---|---|---|
| `let_kind::classify_members` → `MemberKinds` | β | compile_entity passes 1–3, guards.rs, `collect_geometry_exprs`, the skeleton while it exists | Pure function of the member list and `functions`; independent of declaration order; every let and geometry-defaulted param occurrence has exactly one `LetKind`; no reader re-runs the three predicates (R1). |
| `member_support(kind, ctx)` + `unsupported_member_diagnostic` | γ | every member loop over `MemberDecl` in reify-compiler; `MemberDecl` variant additions (reify-ast) | Exhaustive over `MemberKindTag × MemberContext`; `Rejected` always produces an Error-severity diagnostic naming the kind and the context; no context drops a member silently. |
| `lower_purpose_members` ERROR handling | γ | `reify check`, LSP, GUI | An ERROR child of a purpose body yields one `ParseError` at its span. |
| `ReflectiveQueryKind` + `reflective_query_admits` | δ | compile_purpose, reify-ir IR node, engine_purposes fallback, content hash | No String kind remains; `as_str()` spellings and hash bytes unchanged; the fallback match is exhaustive. |
| `PortPath::at_element` | ε | engine_edit.rs per-element materialisation | Output for an `Element` root equals the resolved path's port name for element `i`; a `Literal` index is never rewritten. |
| `SubComponentDecl.origin` | ζ | engine_eval.rs (2 recognisers), resolve_order.rs | Only connect.rs constructs `Connector`; recognisers never inspect the name. |
| `PreRegisterOutput::disposition` | θ | `check_phase_build_available_defaults_map`, `check_phase_inject_defaults` | Both phases consult one value per `(name, kind)`; advertising and injection cannot disagree. |
| `type_acceptance::conformance_accepts` | ι | conformance walker; fn-call, ctor and default sites | One relation; divergences from `type_compatible` are named arms. |
| `joint_self_check::joint_dof_diagnostics` | κ | entities_phase.rs | The only `pub(crate)` item of the module; owns both gates. |
| `RelationSig` table | λ | relation_signatures resolvers, reify-lsp hover (`is_relation_typed_fn`, `is_relation_shared_verb` keep their `pub` signatures), S8 rows | One row per `RelationKind`; every per-relation fact derives from it. |
| `ParseErrorKind::diagnostic_code` | μ | module_dag.rs; S27 `compile_user_module`, reify-lsp `convert_parse_error`; S25 GUI diagnostics | One mapping; render output unchanged. |
| `TopologyTemplate::all_value_cells` | ν | reify-compiler sites; S27 | value_cells ∪ guarded members ∪ else_members, in that order. |

## §6 Boundary-test sketch

| # | scenario | preconditions | postconditions | leaf |
|---|---|---|---|---|
| B1 | forward vs backward geometry alias | `decl_passes_{forward,backward}_geometry_alias.ri` | Both `reify check` → rc 1, identical error line; `cyclic_ident_alias_does_not_crash` green. | β |
| B1b | forward selector alias | let `s = t` before `let t = faces(b)`, used as a selector arg | Same compile result as the backward order. A postcondition guard, not a RED premise: the forward selector order was not probed on main. | β |
| B2 | member-support matrix | every `MemberDecl` kind the grammar admits in each `MemberContext`, as `.ri` source through the compile entry (not hand-built ASTs) | Each cell is compiled, or produces exactly one Error naming kind and context; zero silent cells. | γ |
| B3 | guarded port and meta | `decl_passes_guarded_port_dropped.ri` | rc 1; one Error for the port, one for the meta block. | γ |
| B4 | purpose-body ERROR | `decl_passes_purpose_body_sub_swallowed.ri` | rc 1; a parse error at the `sub` line. | γ |
| B5 | reflective parity | `crates/reify-cli/tests/fixtures/stdlib_sim_ready_{geom_undef,material_active,material_ok}.ri`, `examples/determinacy_intrinsics.ri`, `examples/m5_purpose.ri` | `reify check` stdout+stderr byte-identical to the `8e6ffcac9d` capture (manifest δ). | δ |
| B6 | reflective hash stability | a purpose using each kind | `PurposeReflectiveAggregation` content hash equals the hash computed from the literal string. | δ |
| B7 | deferred connect with a literal index | a `forall v in vents: connect vents[0].<p> -> v.<q>` body (direction-compatible ports; shape after `forall_runtime_re_elaboration.rs`'s Connect fixture) with `n` undef, then `Engine::edit_param(n = 3)` | Connections equal the resolved-path connections for the same source compiled with `n = 3`: `vents[0].<p> → vents[i].<q>` for `i` in 0..3. RED on main per `rewrite_port_placeholder`'s own doc (read, not probed: the deferred path is reachable only through `edit_param`). | ε |
| B8 | connector parity | `examples/auto_binding_sites.ri`; reify-eval connector suites | `reify check` output byte-identical (4 indeterminate today); suites green; R2 = 1. | ζ |
| B9 | user-named `__connector_0` | `sub __connector_0 = Gain()` | Its `SubComponentDecl.origin == Declared`. | ζ |
| B10 | phantom-default cases via source | each of the four same-name-loser shapes behind the skip sets, as `.ri` through `check_trait_conformance` | Same advertised and injected sets as main; no `check_phase_*` call in the test region. | η (pins), θ (stays green) |
| B11 | acceptance sites agree | each deliberate divergence row, at a fn-call, a struct-ctor and a param-default site | Same accept/reject verdict at all three sites. | ι |
| B12 | DOF gates | a joint with a `Type::Error` DOF; a joint with a non-kind DOF type; a joint whose body has `tangent` | No `E_JOINT_DOF_MISMATCH`; the non-kind case gets exactly one `E_ARG_TYPE_MISMATCH`. | κ |
| B13 | relation table | iterate the `RelationSig` table | The names derive from the table; the doc count matches; R3 = 0. (The rot + trans == ΔDOF clause moved to #6669 B4 — lead amendment, D9.) | λ |
| B14 | parse-error kind | a member-continuation source through `compile_project` (module_dag) | Diagnostic carries the mapped code and the original span; CLI `render_errors` text unchanged. | μ |
| B15 | value-cell union | a template with guarded `where`/`else` members | `all_value_cells()` yields value_cells, then each group's members, then else_members. | ν |

## §7 Cross-PRD relationship (G4)

| other PRD / stream | direction | seam mechanism | owner | status |
|---|---|---|---|---|
| S27 tooling front end | produces → | `TopologyTemplate::all_value_cells()` | **S9** (program G4 table) | ν |
| S27 tooling front end | produces → | `ParseErrorKind::diagnostic_code()` for `compile_user_module` and LSP `convert_parse_error` | S9 produces; S27 adopts | μ |
| reify-ast `MemberDecl` variant additions and their dispatch sites | owns | `member_support` table | **S9** (program G4 table) | γ |
| S7 compiler module namespace (`docs/prds/v0_6/compiler-module-namespace.md`) | consumes ← | S7 β (`member_cell_decls.rs`; guards.rs explicit imports), S7 θ (entity.rs explicit imports), S7 ζ (conformance/ explicit imports); lib.rs `mod` lines for `let_kind`, `member_support`, `type_acceptance` follow S7 D3 (top-level siblings importing downward) | S7 | ξ depends on S7 β and θ; ο on S7 ζ; every S9 leaf lands after S7 per the program's compiler order |
| S6 kinematics | consumes ← | `RelationKind` in reify-ir | S6 | λ #8741 HELD (`x_seam_wait`) until S6's RelationKind leaf is filed and wired |
| S8 builtin registry | produces → | `RelationSig` relation facts that reify-builtins rows read | S9 (relation semantics); S8 (row schema) | λ first (wave order) |
| S4 cell-eval closure | produces → | engine_edit.rs per-element connect materialisation via `PortPath::at_element`; engine_eval.rs connector recognisers on `SubOrigin` | S9 edits first; S4 partitions those files in wave 2 | ε, ζ |
| uniform-member-access (`docs/prds/v0_6/uniform-member-access.md`) | consumes ← | #5425 post-acceptance `is_geometry_let`; #5430 owns member_path consumption | that PRD | β depends on #5425 |
| stdlib-namespace (`docs/prds/v0_6/stdlib-namespace.md`) | consumes ← | #5494 deletes the skeleton (β-compatible); #5499 owns stdlib import edges | that PRD | reference |
| S12 check surface | produces → | S12 lists S9 upstream; S9's contracts are §5 | S9 | the lead names the S12-required contract if it is outside §5 |

## §8 Decomposition plan

Each leaf carries `metadata.x_finding_key` (list), `x_finding_run = "review-all-reify-20261010"`,
`source = "review-all"` and priority = severity. A task saying "extract X" names its ratchet row.
New gate-resident tests carry their drift-guard registrations in the same diff (overlay
"Gate-test drift-guard registration").

- **α #8730 — Construct-ratchet member for the declaration passes** *(intermediate → β, ζ, λ, ν)*.
  Modules: `tests/infra/test_compiler_decl_pass_ratchets.{py,sh}`,
  `tests/infra/run-all-classification.manifest`. Ships R1–R3 at today's baselines (17 / 4 / 10),
  with hermetic flag/clear fixtures. Signal: the member runs under `run_all.sh`, its fixture cases
  red on an over-baseline file and green on the clean twin, and it is green on main. Priority
  medium. Keys: none (infrastructure for INV candidate 5).
- **β #8731 — Classify each let once: `LetKind` / `MemberKinds`** *(leaf)*. Modules: reify-compiler
  `let_kind.rs` (new), entity.rs, guards.rs, geometry_list.rs as needed. D1. Signal: B1, B1b; R1 at
  1; `compile_entity`'s four prologue sets gone; the function doc no longer says "Two-pass".
  Deps: α, #5425. Priority **high**. Keys: fk-0bdf9598b402.
- **γ #8732 — One member-support table; no silent member drops; purpose-body parse errors reported**
  *(leaf)*. Modules: reify-compiler `member_support.rs` (new), entity.rs, guards.rs, traits.rs,
  compile_builder/sketch_unsupported.rs; reify-syntax ts_parser.rs (`lower_purpose_members`);
  reify-ast decl.rs (`MemberDecl::tag`). D2. Signal: B2, B3, B4; the 11 duplicate strings and
  `unsupported_purpose_member_info` gone. G7: establishes INV-SF-3 for member positions. Priority
  medium. Keys: fk-196deab564e7.
- **δ #8733 — `ReflectiveQueryKind` end to end** *(leaf)*. Modules: reify-ir expr.rs; reify-compiler
  types.rs, traits.rs, expr.rs; reify-eval engine_purposes.rs; reify-test-support. D3. Signal: B5,
  B6; no `query_kind: String`. Priority medium. Keys: fk-30505ec518b4, fk-ee1be91862ca.
- **ε #8734 — Typed `PortPath` for deferred forall-connect endpoints** *(leaf)*. Modules: reify-compiler
  types.rs, forall_elaborate.rs, connect.rs (`resolve_port_name` callers); reify-eval engine_edit.rs.
  D4. Signal: B7 (RED first on main: the literal endpoint is rewritten); `rewrite_port_placeholder`
  deleted; no Info string cites a task number; the module doc states that Connect is materialised
  at runtime. Priority medium. Keys: fk-27489259525d.
- **ζ #8735 — `SubOrigin` on `SubComponentDecl`** *(leaf)*. Modules: reify-compiler types.rs, connect.rs
  and the `SubComponentDecl` construction sites; reify-eval engine_eval.rs, resolve_order.rs;
  reify-test-support builders. D5. Signal: B8, B9; R2 at 1. Deps: α. Priority medium. Keys:
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
  `type_acceptance.rs` (new), conformance/mod.rs. D7. Signal: B11; the walker's own-rule arms
  delegate; its divergence prose is deleted, with a one-line pointer per named arm. Deps: η.
  Priority medium. Keys: fk-37775e022693.
- **κ #8740 — One gated DOF entry point** *(leaf)*. Modules: joint_self_check.rs,
  compile_builder/entities_phase.rs. D8. Signal: B12; one `pub(crate)` item in the module; the
  entities_phase §(4) block is one call. Priority medium. Keys: fk-cc676a29f5d9.
- **λ #8741 — `RelationSig` table keyed by `RelationKind`** *(leaf; was HELD on S6; released 2026-10-13 by S6's session with dependency S6 γ #8830 — RelationKind parsed at reify-eval relate_solve::relation_instance (S6 R4); λ keys on `RelationKind::from_name`)*. Modules: relation_signatures.rs (and
  its LSP callers' imports, if signatures move). D9. Signal: B13; R3 at 0; the tests iterate the
  table. Deps: α, κ, S6 γ #8830. Priority medium. Keys:
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
