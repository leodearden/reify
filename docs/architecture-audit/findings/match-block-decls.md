# Audit: Same-Name Guarded Declarations from `match` Blocks

**PRD path:** `docs/prds/match-block-decls.md`
**Auditor:** audit-match-block-decls
**Date:** 2026-05-12
**Mechanism count:** 14
**Gap count:** 6

## Top concerns

- **Surface syntax is not parseable.** Tree-sitter grammar has only `match_expression` (expressions); no rule for declaration-level `match { ... => sub head : ... }`. The `MemberDecl::MatchArmDeclGroup` AST node is fully implemented and tested, but every test hand-constructs it. No `.ri` source file can exercise this feature end-to-end through the parser today. Example `examples/m5_guarded_head_type.ri` uses `where`/`else`, not the match-block syntax.
- **PRD example syntax outruns implementation.** The PRD shows arms with bodies and `where` clauses (`sub head : HexHead { ... }`); `compile_match_arm_decl_group` (entity.rs:2510-2521) emits an explicit "where clauses and bodies are not yet supported in match-arm sub declarations" error. So even if the parser produced the variant, the PRD's own example would diagnose.
- **Narrowing path is dormant, not wired.** `narrow_arms_under_guard` exists (guards.rs:674) and has unit-tested contract semantics, but every call site (`expr.rs:1167`) passes `current_guard = None`. The feature ships the *full union* at every reference site. The `#[allow(dead_code)]` annotation on the narrow helper confirms this is intentional v0.1 scope.
- **Reference safety from outside the match (acceptance criterion 7) is not implemented.** The PRD requires rejecting `bolt.head` from a `where` block whose condition contradicts every arm's guard. No code performs that check; only the standard guarded-decl reference-safety sweep runs, and arm guards are emitted as ordinary `__guard_N` cells without an outside-implication probe.
- **Indexed-access on collection subs is partial.** Cluster-aware lookup works for literal-index access `bolts[0].head.x` (task 2871, expr.rs:1437-) but variable-index `bolts[i].head.x` silently merges arm member maps (task 2870, status `pending`).

## Mechanisms

### M-001: Decl-level `match` block surface syntax (tree-sitter grammar)

- **State:** FICTION
- **Failure mode:** F1 (compile-time contract → no parser backing)
- **Evidence:** `tree-sitter-reify/grammar.js:702-720` defines only `match_expression`, no decl-level match block rule. Mem0 observation `db1273a1-eae2-43a8-be3a-3f29fbb9f315`: "match-block decls (spec §6.4) are NOT yet representable in the tree-sitter grammar". No reify-syntax test parses match-block source; all tests hand-construct the AST (`crates/reify-syntax/tests/match_decl_block_tests.rs:1-10`).
- **Blocks:** Acceptance criteria 1, 2, 3, 5, 6, 7, 8 — every AC requires the parser to admit the syntax.
- **Note:** AST node and downstream compiler infrastructure exist; only the grammar production + ts_parser lowering function is missing. This is the binding constraint between the PRD's surface examples and the existing implementation.

### M-002: `MemberDecl::MatchArmDeclGroup` AST representation

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-syntax/src/lib.rs:297-302, 344-348, 392-402`; tests `crates/reify-syntax/tests/match_decl_block_tests.rs:46-194`; task 2372 done (commit 719782507772).
- **Note:** Carries discriminant expr, ordered arms (each with patterns Vec, boxed member), span, content_hash. Visitor walkers descend into arms.

### M-003: Tree-sitter → AST lowering (`lower_match_arm_decl_group`)

- **State:** FICTION
- **Failure mode:** F1 (no producer for the AST variant from real source)
- **Evidence:** `crates/reify-syntax/src/ts_parser.rs` has `lower_match_expr` and `lower_match_arm` (line 2335, 2358) but no decl-level equivalent; consumer-side references to `MemberDecl::MatchArmDeclGroup` (lines 2757, 2943, 3075, 3134, 3247, 3266) appear only in serializers/walkers — none construct the variant.
- **Blocks:** Same as M-001; tightly coupled.
- **Note:** Even if grammar.js were extended, a lowering function would still need to map the tree-sitter nodes to `MatchArmDeclGroupDecl`/`MatchArmDeclArmDecl`. Tests circumvent this with hand-constructed AST.

### M-004: `GuardedDeclGroup` / `GuardedDeclArm` compiler symbol-table type

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/types.rs:706-749, 738`; `crates/reify-compiler/src/scope.rs:92, 109, 123, 229-246`; task 2372 done. Unit tests `types.rs:1305-1322`. Producer-side invariant (`match_arm_groups.keys() == match_arm_group_arm_member_types.keys()`) enforced by `assert!` in `compile_entity` (task 2872).
- **Note:** Held separately from regular `names` map so outside-match collision checks don't misfire. Persisted on `TopologyTemplate.match_arm_groups`.

### M-005: Compile `MatchArmDeclGroupDecl` → cluster + per-arm `SubComponentDecl`s

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/entity.rs:2257-2649` (`compile_match_arm_decl_group`). Synthesizes per-arm guards (`discriminant == EnumType.Variant`, OR-chains for `|`-pipe arms), allocates synthetic `__guard_N` ValueCells, emits a `SubComponentDecl` per Sub arm with `GuardState::Compiled`, and pushes empty `CompiledGuardedGroup`s so the guard cells participate in the reference-safety sweep. Trait bound + arg-conformance checks queued via `PendingBoundCheck::SubComponent` / `TraitArgConformance` (entity.rs:2569, 2577).
- **Note:** Mirrors the spec §6.4 `where` desugaring. Per-arm child-template member maps captured into `match_arm_group_arm_member_types`.

### M-006: Match-arm Sub `where` clause + body support

- **State:** PARTIAL
- **Failure mode:** F2 (PRD background example exceeds implementation)
- **Evidence:** `crates/reify-compiler/src/entity.rs:2506-2521` emits explicit error "where clauses and bodies are not yet supported in match-arm sub declarations". PRD background line 9 shows `sub head : HexHead { ... }` with a body.
- **Blocks:** Faithful surface use of PRD's headline example.
- **Note:** No follow-up task tracks adding body/where support inside match arms; only the inline diagnostic. Acceptance criterion 1 (which says the example "parses and elaborates with no duplicate-name error") would be partially satisfied — duplicate-name suppression works, but the body content is rejected.

### M-007: Exhaustiveness gate on discriminant enum variants

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `entity.rs:2436-2468` — flattens all arms' patterns (including pipe alternatives) and emits "non-exhaustive match on '<Enum>': missing variant(s) ..." if any variant uncovered. Early-return prevents partial cluster registration. Task 2375 done (commit bc14c49057c1).
- **Note:** Reuses no shared infrastructure with expression-level match exhaustiveness — independent walker. PRD scope says "piggyback on existing match-expression exhaustiveness"; the implementation chose duplication. **DRIFT candidate** (mild). Plus unknown-enum case explicitly skips the gate (`if let Some(variants) = known_enum_variants`) — covered by task 2376.

### M-008: Variant-pipe arm patterns (`Hex | Button => sub head : ...`)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `MatchArmDeclArmDecl.patterns: Vec<String>` (`crates/reify-syntax/src/lib.rs`); guard synthesis builds OR-chain (`entity.rs:2489-2495` via `build_arm_guard_expr`); pattern validation iterates `arm.patterns` (`entity.rs:2421-2434`); test `match_arm_decl_group_variant_pipe_arm_carries_multiple_patterns` (`match_decl_block_tests.rs:81-90`).
- **Note:** Fully supported in the AST + compiler; blocked only by M-001 from real-source use.

### M-009: Outside-match same-name collision diagnostic

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `entity.rs:423-448` declares `seen_match_arm_cluster_names`, `match_arm_cluster_logical_names`, `clusters_with_outside_collision`, `outside_decl_spans`. Forward direction (`Sub`/`Param`/`Let` before the match — `entity.rs:520-529, 549-559, 845-857`) and reverse direction (match before outside decl — same scaffolding via `match_arm_cluster_logical_names`) both call `emit_outside_match_collision`. Cluster suppressed via `clusters_with_outside_collision` short-circuit (`entity.rs:2367`). Task 2375 + 2376 done.
- **Note:** PRD scope §6 says "outside-match same-name decl — diagnose"; this is implemented. **Known scope limitation** (intentional, documented in `entity.rs:440-448`): names registered through `GuardedGroup` children are NOT tracked, so `where g { param head ... } else { ... }` colliding with a cluster doesn't diagnose. Filed as "future task (option a)".

### M-010: Union typing on `self.<cluster>` MemberAccess

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/expr.rs:1167-1174`. Returns `Type::Union(arm_types)` via a synthetic `__match_arm_group_<member>` ValueRef. `Type::Union` defined in `reify-types/src/ty.rs:130-146` as compile-time-only (rejected by `is_representable_cell_type`).
- **Note:** Task 2373 done.

### M-011: Cluster-inner member resolution `self.<cluster>.<inner>` / `<sub>.<cluster>.<inner>`

- **State:** PARTIAL
- **Failure mode:** F3 (partial coverage; one important call shape silently merges)
- **Evidence:** `expr.rs:1212-1250` (self path), `expr.rs:1379-1415` (external sub path), `expr.rs:1437-` (collection-sub literal-index path, task 2871). Helper `resolve_cluster_inner_member` enforces "field present in every arm with compatible type" else diagnoses with offending arm list (task 2374 done). Task 2870 (variable-index `bolts[i].head.x` on collection subs) is **pending** — silently routes through merged last-arm-wins `sub_member_types` map.
- **Blocks:** Acceptance criterion 2 partial — common-vs-arm-specific field resolution works for self / direct-sub / literal-indexed collection access; not for variable-indexed collection access.
- **Note:** Missing-field diagnostic with arm list (PRD task 3 / AC 2) is wired. The acceptance criterion does not name variable indexing, but the dynamic-index gap is a silent footgun for the broader feature.

### M-012: Guard-narrowing under arm guard at reference site

- **State:** PARTIAL
- **Failure mode:** F4 (helper exists, never invoked productively)
- **Evidence:** `crates/reify-compiler/src/guards.rs:651-704` — `narrow_arms_under_guard` carries `#[allow(dead_code)]`; doc comment says "callers in `expr.rs` always pass `None`". `expr.rs:1163-1166` confirms: "for now we always return the full union (correct when `current_guard == None`, which is the common case for v0.1 surface syntax)". Unit-test contract pinned (`guards.rs:706-859`) but no integration test narrows.
- **Blocks:** Acceptance criterion 2 (narrow access of arm-specific field under `where head_type == HeadType.Hex` guard) — partially, because nothing in surface syntax can produce a `current_guard` that matches an arm cell.
- **Note:** Decision: ship full-union typing for v0.1, defer narrowing to a future task when surface syntax for "narrowing-on-decl" is introduced. Coupled to M-014 (general guard implication checker).

### M-013: Reference safety from outside the match (PRD AC 7)

- **State:** FICTION
- **Failure mode:** F5 (PRD-required check has no code)
- **Evidence:** Grep across `crates/reify-compiler/src` for "outside-match ref", "contradicts every arm", "every arm's guard" returns zero hits. The arm `__guard_N` cells participate in the standard reference-safety sweep via empty `CompiledGuardedGroup` records (entity.rs:2611), but no logic specifically detects "guard at reference site is incompatible with all arm guards". PRD AC 7: "accessing `bolt.head` from a `where` block whose condition contradicts every arm's guard is rejected."
- **Blocks:** Acceptance criterion 7.
- **Note:** Likely depends on a general guard-implication / SAT-style check (see M-014). PRD §6.3 / §8.10 reference an "implication check" infrastructure that the PRD assumes is "already present" — see M-014.

### M-014: Generic guard-implication checker (`§8.10`)

- **State:** PARTIAL
- **Failure mode:** F6 (PRD assumes infrastructure broader than what exists)
- **Evidence:** PRD line 12: "Reference safety (spec §6.3 / §8.10): a reference to `head` from outside the `match` is valid only when the referencing context's guard implies that *some* arm is active." Implementation has only `narrow_arms_under_guard`'s parent-chain walk (`guards.rs:651-704`), which is purely structural (lexical parent guards) — not a semantic implication checker over arbitrary boolean expressions. Grep for `implication`, `implies`, `implied_by` in reify-compiler/src returns only the conservative parent-chain walk in `narrow_arms_under_guard` and an unrelated comment in `type_compat.rs:122`.
- **Blocks:** M-012 narrowing under semantically-implied guards (e.g. `where head_type == HeadType.Hex` ⇒ Hex arm only); M-013 outside-match reference safety; spec §8.10 in general.
- **Note:** This is the most consequential gap because the PRD repeatedly leans on "the existing implication check". The reality is that v0.1 has parent-chain walking only — no SMT, no expression-level boolean implication. Any narrowing or outside-match contradiction detection beyond lexical parent-arm reachability is unwritten.

## Skipped or boundary mechanisms (not gaps)

The PRD explicitly defers these (out-of-scope, §"Out of scope"); they are not classified as gaps:

- Same-name decls from non-`match` mutually-exclusive guards (e.g. negation pairs).
- Pattern-matching with payload-bound names (v0.1 enums are C-style).
- Cross-arm unification for differently-named decls.

## Cross-PRD breadcrumbs

- **`shadowing-warning.md`** (`docs/prds/shadowing-warning.md:10`) explicitly delineates §6.4 same-name match decls as a distinct case from §8.8 trait merge collisions. Coupling: any change to the cluster shape interacts with shadowing diagnostics.
- **`auto-type-param-resolution.md`** references §8.10 in the same spec section the match-block PRD leans on. The "existing implication check" (M-014) is presumed by both PRDs — if it is in fact a gap, both PRDs share the dependency.
- **`specialization-scope.md`** — `MatchArmDeclGroup` recursion is wired into `walk_specialization_scope_members` (lib.rs:344-348). Specialization-scope validation tests already cover the nested-sub case (`crates/reify-compiler/tests/specialization_scope_validation_tests.rs:14-124`).
- **`forall-statement-form.md`** has the same shape gap (grammar rule + tree-sitter lowering for a statement-form construct) — see related Mem0 entries about `forall_statement` being in the grammar even though decl-level match is not. Different outcome despite similar pattern.

## Cited tasks (for Phase 3 cross-referencing)

| Task | Status | Role |
|---|---|---|
| 2371 | done | LSP integration (specialization-scope, not match) |
| 2372 | done | `GuardedDeclGroup` + AST variant + walkers |
| 2373 | done | union typing + `narrow_arms_under_guard` (dormant) + per-arm member maps |
| 2374 | done | missing-arm-field diagnostic |
| 2375 | done | exhaustiveness gate + outside-match collision |
| 2376 | done | test suite + precedence pinning |
| 2612 | done | empty-arm registration gate + register API |
| 2613 | done | duplicate-cluster pre-pass guard |
| 2869 | done | dedup empty-per_arm diagnostic |
| 2870 | **pending** | variable-index `bolts[i].head.x` cluster-aware lookup |
| 2871 | **pending** | orphan `match_arm_group_arm_member_types` entries on logical-name mismatch (latent inconsistency) |
| 2872 | done | atomic per-arm member map insert (invariant enforcement) |
| 2877 | done | scope notes (outside_decl_spans documentation) |
| 3045 | done | extra typing test coverage |

## Termination note

Hard cap respected; final summary memory written; per-gap memories written for M-001, M-003, M-006, M-011, M-012, M-013, M-014 (the seven non-WIRED rows after rolling M-007's DRIFT-candidate observation into the note rather than its own gap entry).
