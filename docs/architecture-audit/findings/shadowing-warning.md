# Audit: Shadowing Warning

**PRD path:** `docs/prds/shadowing-warning.md`
**Auditor:** audit-shadowing-warning
**Date:** 2026-05-12
**Mechanism count:** 17
**Gap count:** 6

## Top concerns

- **Acceptance criterion #1 ("sub-structure body") is unverifiable today.** PRD AC #1 says: "Declaring `param x` in a sub-structure body when an enclosing structure already has `param x` emits warning W_SHADOW with both spans." Reify's parser hardcodes `SubDecl.body = None` at `crates/reify-syntax/src/ts_parser.rs:1707` ("Grammar does not yet produce specialization-scope bodies; see SubDecl docs and task 2368 plan"). The AST field exists (`SubDecl.body: Option<Vec<MemberDecl>>` at `crates/reify-syntax/src/lib.rs:220`) and the shadow_lint module visits `MemberDecl::Sub(s)` walking `s.args` and `s.where_clause` but never recurses into `s.body`. So even if the parser produced a spec-body, the lint would still miss the case. The PRD's §Scope ("nested specialization scopes") is therefore PARTIAL via parser AND DRIFT via the lint (silent gap, no TODO marker in `shadow_lint.rs` flagging this).
- **`#[allow(shadowing)]` suppression syntax does not match the in-repo annotation framework.** PRD: "Lint-style: warning by default, suppressible via `#[allow(shadowing)]` only if/when annotation framework lands". An annotation framework HAS landed (`@test`, `@optimized`, `@solver_hint`, `@shell`, `@solid` — see `crates/reify-compiler/src/annotations.rs` and `crates/reify-types/src/annotation.rs`), but it uses `@name(args)` syntax, not `#[allow(name)]` (Rust-style brackets). PRD literal syntax is unimplemented and would require either grammar extension or a re-spelling decision; no task tracks either. TODO.
- **Type-alias collision target listed in PRD scope is structurally unreachable.** PRD §Scope: "checks for collision against parameters, ports, sub-entities, `let` bindings, and **type aliases**." `TypeAlias` exists only as `Declaration::TypeAlias` at module scope (`crates/reify-syntax/src/lib.rs:30`) — never as a `MemberDecl`. Modules have no parent scope to shadow into, so the language has no syntactic position where a child decl could shadow a type alias. The PRD over-specifies. DRIFT, harmless in practice; flagged here because a Phase-3 review of PRD wording will see "type aliases" and may want either to drop the line or to introduce module-scope shadow detection (currently absent — see M-014).
- **Lint is otherwise unusually well-covered.** 23 dedicated tests in `crates/reify-compiler/tests/shadowing_warning_tests.rs` (1722 LoC) + 1 LSP end-to-end test + diagnostic-code round-trip tests in `reify-types`. Lambda/Quantifier/Forall binders, fn-body lets, purpose-body lets, port-internal scopes, trait-merge no-warn, match-arm no-warn, guarded-group sibling semantics, multi-hop grandparent, and diagnostic-message format are all pinned. Single-pass scope walker is invoked from the unified `compile_with_prelude_context` entry point at `crates/reify-compiler/src/lib.rs:255`, so every compile flow gets the lint.

## Mechanisms

### M-001: Single-pass AST scope-walk lint pass

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/compile_builder/shadow_lint.rs:98-102` (`lint_module`); invoked at `crates/reify-compiler/src/lib.rs:255` between `dot_chain_lint::lint_module` and `specialization_scope_check::validate_module` so every compile flow runs it.
- **Note:** Walks `parsed.declarations` once. PRD task #1 ("Implement single-pass scope-walk shadow detector").

### M-002: `FrameStack` linked-list scope model (lookup walks innermost-to-outermost)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `shadow_lint.rs:74-89` — `FrameStack { frame, parent }` lives on the call stack; `lookup` returns first match. Replaces an earlier `&[&Frame]` design (per inline comment, avoiding per-recursion `frames.to_vec()` allocation in the lambda/quantifier hot path).
- **Note:** "Nearest visible parent" rule for AC #3 (multi-hop) is implemented here; pinned by `nested_lambda_shadow_points_at_nearest_visible_parent` at `tests/shadowing_warning_tests.rs:147`.

### M-003: Per-declaration-arm body-frame construction (Structure / Occurrence / Trait / Function / Purpose / Constraint / Field)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `shadow_lint.rs:106-252` (`walk_declaration` match). Structure/Occurrence/Trait collect a body frame via `collect_body_frame`; Function/Purpose use the shared `walk_child_scope_body` helper (params as outer, body names as inner); Constraint builds a single param frame; Field walks expressions under an empty top-level frame so lambdas catch shadows on their own.
- **Note:** Six of seven member-bearing decl kinds are covered. Enum/Unit/TypeAlias are explicit pass-throughs (`shadow_lint.rs:248-251`), correct because they don't introduce expression scopes — but see M-014 for the corollary missing coverage at module scope.

### M-004: `collect_body_frame` (param / let / sub / port-name / guarded-group sibling)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `shadow_lint.rs:270-342`. Params, lets, subs, ports (by port name only), and guarded-group members all fold into one frame with first-seen semantics (`entry().or_insert`). Recursion is bounded by `reify_syntax::MAX_MEMBER_NESTING_DEPTH`. ForallConnect/ForallConstraint/Constraint/etc. are explicitly pass-through (forall variable is bound in the body, not the parent frame).
- **Note:** First-seen semantics + sibling-group fold pinned by `guarded_group_shadow_original_decl_uses_first_seen_branch` at `tests/shadowing_warning_tests.rs:1575`.

### M-005: Lambda-binder shadow detection (`ExprKind::Lambda`)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `shadow_lint.rs:515-532`. Pushes child frame, emits one warning per param that collides with any enclosing frame. Test: `lambda_param_shadows_entity_param_emits_w_shadow` at `tests/shadowing_warning_tests.rs:16`.

### M-006: Quantifier-binder shadow detection (`ExprKind::Quantifier`)

- **State:** PARTIAL
- **Failure mode:** F1 (minor — span granularity differs from ideal)
- **Evidence:** `shadow_lint.rs:533-559`. Collection walked in outer scope, then variable checked against frames before pushing a one-element child frame. **Span granularity TODO**: child label uses `expr.span` (the whole `forall x in coll: pred`) rather than a dedicated `variable_span` field because `ExprKind::Quantifier` doesn't yet carry one — see explicit TODO at `shadow_lint.rs:545-551`. Test: `quantifier_variable_shadows_entity_param_emits_w_shadow` at `tests/shadowing_warning_tests.rs:76`.
- **Blocks:** editor-squiggly UX improvement (not blocking PRD acceptance — only the warning fires; the wider-than-ideal span is documented).
- **Note:** PRD AC does not specify span granularity; treated as a TODO/refinement, not a gap against acceptance.

### M-007: ForallConnect / ForallConstraint binder shadow detection

- **State:** PARTIAL
- **Failure mode:** F1 (same span-granularity caveat as M-006)
- **Evidence:** `shadow_lint.rs:457-484` + helper `walk_forall_binder` at `:709-734`. Child-label span is `f.span` (the outer ForallXDecl span); same TODO as M-006 awaiting a dedicated `variable_span` field on the AST node. Tests: `forall_connect_variable_shadows_entity_param_emits_w_shadow` at `:1482`; `forall_constraint_variable_shadows_entity_param_emits_w_shadow` at `:1662`.

### M-008: Function- and Purpose-body let-shadows-param via `walk_child_scope_body`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `shadow_lint.rs:141-247` + helper `:658-688`. Both arms route through `walk_child_scope_body(params, body_names, walk_body)`. Tests: `fn_body_let_shadows_fn_param` at `:911`, `purpose_body_let_shadows_purpose_param` at `:994`, parity test `fn_and_purpose_body_arm_emit_analogous_shadow_warnings` at `:1092`.

### M-009: Port-internal scope as child of enclosing entity body

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `shadow_lint.rs:426-442`. Pushes a port frame onto the stack before recursing into port members so lambda params inside `port p { param q ; let f = |q| q }` see port-internal binders as a parent scope. Test: `lambda_in_port_member_shadows_port_internal_binder` at `:1414`.

### M-010: Trait-merge exclusion (single-source iteration, §8.8)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `shadow_lint.rs:109-122` (Structure arm walks ONLY `s.members`); module-doc rationale at `:35-43`. Single-source-iteration means trait member sets are never injected — automatic exclusion, no explicit filter required. Test: `trait_merged_member_does_not_warn` at `tests/shadowing_warning_tests.rs:326`.

### M-011: Match-arm same-name guarded-decl exclusion (§6.4)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `shadow_lint.rs:339` and `:487` — `MemberDecl::MatchArmDeclGroup(_)` is an explicit pass-through (no frame contribution, no body walk). Test: `match_arm_style_guarded_subs_do_not_warn` at `tests/shadowing_warning_tests.rs:285`. Spec §6.4 carve-out from PRD AC #4 is honored.

### M-012: Import-name exclusion (§8.11)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `shadow_lint.rs:133-140` — `Declaration::Import(_)` matched explicitly and discarded; no module-scope frame aggregates imports. Test: `imported_name_does_not_form_parent_scope` at `tests/shadowing_warning_tests.rs:370`. PRD §"Out of scope" explicitly carves this out.

### M-013: `DiagnosticCode::Shadowing` enum variant + PascalCase wire serialization

- **State:** WIRED
- **Failure mode:** N/A (with DRIFT on PRD's literal name `W_SHADOW`)
- **Evidence:** Variant declared at `crates/reify-types/src/diagnostics.rs:340`; round-trip + Debug-print pinned at `:1247-1259`; LSP wire-form `"Shadowing"` pinned at `crates/reify-lsp/src/convert.rs:425` in the PascalCase-stability table. PRD §Scope says "New diagnostic code (e.g. `W_SHADOW`)" — the "e.g." hedge makes this DRIFT not FICTION, but external tooling grep'ing for `W_SHADOW` will find nothing in the codebase.
- **Note:** Same pattern as `DeepDotChain` (audit-deep-dot-chain M-004); the PRD's `W_*` prefix convention does not match the in-repo `DiagnosticCode::PascalCase` convention.

### M-014: Diagnostic shape — message + two labels (child site + original decl site)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `shadow_lint.rs:753-773` (`push_shadow_diagnostic`). Canonical message `"declaration of '<name>' shadows enclosing declaration"`; child-label `"shadows the enclosing declaration"`; original-decl label `"originally declared here"`. Wording pinned by `shadow_diagnostic_message_format_is_pinned` at `tests/shadowing_warning_tests.rs:790`. LSP `related_information` carries the original-decl label (compiler-side + LSP-side both tested).

### M-015: `#[allow(shadowing)]` suppression annotation

- **State:** TODO (PRD-acknowledged as conditional)
- **Failure mode:** F1 (PRD suggests a mechanism gated on "if/when annotation framework lands")
- **Evidence:** PRD §Scope: "Lint-style: warning by default, suppressible via `#[allow(shadowing)]` only if/when annotation framework lands; otherwise plain warning." `grep -rn '@allow\|allow.*shadow\|#\[allow' crates/reify-compiler/src` returns no matches relevant to the language layer. `@`-style annotation framework exists for `@test`/`@optimized`/`@solver_hint`/`@shell`/`@solid` (`crates/reify-types/src/annotation.rs:1-44`) but uses `@name(args)` syntax, not Rust-style `#[allow(name)]`.
- **Blocks:** none currently (PRD self-gates this on a future decision); but if a future user-facing feature requests shadow-suppression, the wire syntax (`@allow_shadowing` vs `#[allow(shadowing)]` vs other) is undecided.
- **Note:** Spelling-mismatch is the meat of the gap. The PRD's literal `#[allow(shadowing)]` Rust-bracket form would require either grammar extension or a re-spelling agreement.

### M-016: Sub-entity / specialization-scope body shadow detection

- **State:** PARTIAL (parser side missing; lint side silently absent)
- **Failure mode:** F1 (PRD assumes a syntactic form the parser doesn't produce, and the lint silently does not handle the AST shape that IS reserved for it)
- **Evidence:** PRD AC #1: "Declaring `param x` in a sub-structure body when an enclosing structure already has `param x` emits warning W_SHADOW with both spans." PRD §Scope: "Apply to: ... and nested specialization scopes." AST has the field (`SubDecl.body: Option<Vec<MemberDecl>>` at `crates/reify-syntax/src/lib.rs:220`), parser hardcodes `body: None` at `crates/reify-syntax/src/ts_parser.rs:1705-1707` ("Grammar does not yet produce specialization-scope bodies; see SubDecl docs and task 2368 plan"). Shadow lint's `MemberDecl::Sub(s)` arm walks `s.args` and `s.where_clause` only (`shadow_lint.rs:398-408`) — does NOT recurse into `s.body` even if present. No TODO marker in shadow_lint flags this gap; the lint will silently fail to fire on the AC-#1 case once the parser is wired.
- **Blocks:** PRD AC #1 cannot be tested; depends on task 2368 plan (specialization-body parsing).
- **Note:** Cross-references the broader `specialization_scope_check.rs` module which validates rules for the same syntactic form. Spec §8.7 ("Specialization Scopes") is the contract this AC depends on.

### M-017: Module-scope shadow detection (across `Declaration::*` siblings)

- **State:** ORPHAN / out-of-scope (informational)
- **Failure mode:** N/A
- **Evidence:** `shadow_lint.rs:106` iterates `parsed.declarations` but never builds a module-level frame — each top-level decl is walked independently. PRD §Scope does not list "module" as a shadow site, and §"Out of scope" carves out imported-name shadowing (the only realistic module-scope case). No gap.
- **Note:** Flagged only because M-014's "type aliases" listing might lead a future reader to expect module-scope handling. Phase-3 PRD-wording cleanup may want to drop "type aliases" from the PRD-Scope list (see top concerns).

## Cross-PRD breadcrumbs

- **`docs/prds/match-block-decls.md`** is referenced by PRD §Background and the lint's MatchArmDeclGroup carve-out (M-011). That PRD's §"Notes" says: "Same-name decls from non-`match` mutually-exclusive guards ... May diagnose as duplicate or as shadowing for now." — meaning manual `where { } else { }` shadowing-vs-duplicate semantics are deferred there. Coupling exists but no contradiction.
- **`docs/prds/specialization-scope.md`** is the home of the `SubDecl.body` parser feature on which M-016 is gated. The `specialization_scope_check.rs` module lives in the same crate and is called immediately after `shadow_lint::lint_module` at `crates/reify-compiler/src/lib.rs:256-259` — but it validates structural rules for spec scopes, not shadowing.
- **`docs/prds/deep-dot-chain.md`** is the sibling lint that pioneered the `FrameStack` allocation-avoidance pattern (per `shadow_lint.rs:71-73`) and the `MAX_EXPR_DEPTH = 256` guard (`:50-57`). PascalCase diagnostic-code wire-string pattern is also shared. Cross-cutting "lint hygiene" conventions worth Phase-3 attention.

## Boundaries respected

- One PRD per agent. Cross-PRD references noted, not chased.
- Read-only. No edits to code, tests, tasks, or PRDs.
- No fixes proposed. Phase 3 owns disposition.
- No re-research of seeded gaps; none of GR-001's seeded gaps surface in this PRD's mechanism set.
