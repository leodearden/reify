# Audit: Specialization-Scope Validation

**PRD path:** `docs/prds/specialization-scope.md`
**Auditor:** audit-specialization-scope
**Date:** 2026-05-12
**Mechanism count:** 15
**Gap count:** 2

## Top concerns

- **The parser never produces a specialization-scope body.** The grammar (`tree-sitter-reify/grammar.js:470-494`) supports only `sub name = StructName(args)` and `sub name : List<StructName>`; the `sub name : Type { body }` form named in the spec (§8.7, example at lines 1608-1619) and every PRD acceptance criterion does not parse. `ts_parser.rs:1707` hardcodes `body: None`. Every validator/walker/diagnostic that was built is unreachable from real Reify source today — only AST-builder unit tests exercise it.
- **No task tracks the grammar update.** Tasks 2368-2371 (validator, diagnostic, tests, LSP) are all `done`/merged. Search of fused-memory for the deferred grammar work returns nothing specific to `sub name : Type { body }`; the SubDecl docstring (`lib.rs:217-219`) and parser comment (`ts_parser.rs:1705-1706`) merely note "future grammar update" with no task ID. This is the only PARTIAL/FICTION risk surface in the PRD.
- **Match-arm decl groups are also parser-unreachable.** `MatchArmDeclGroup` (the AST shape that would carry `match`-arm `sub` blocks per the PRD's "applies anywhere a specialization scope appears" clause) has no grammar rule (`grep match_arm_decl_group tree-sitter-reify/grammar.js` is empty). All compiler-side handling (walker `lib.rs:344-347`, integration test `specialization_scope_validation_tests.rs:128-175`) builds the variant by hand. Same shape of FICTION as mechanism 2.
- **`forall ... : <body>` cannot open a specialization scope.** Grammar restricts forall bodies to `connect_statement | chain_statement | constraint_declaration | constraint_instantiation` (`grammar.js:676-688`); none of those produce a `SubDecl` with a body. So the PRD's "if they generate specialization scopes" clause is vacuously satisfied today and the LSP/validator work is uniform; no gap here.

## Mechanisms

### M-001: Specialization-scope AST discriminator (`SubDecl.body.is_some()`)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-syntax/src/lib.rs:208-223` (the `body: Option<Vec<MemberDecl>>` field IS the spec §8.7 flag, per docstring); regression tests `crates/reify-syntax/tests/sub_decl_specialization_tests.rs:48-65`
- **Blocks:** none
- **Note:** The AST representation chosen — distinguishing specialization scopes by `body: Some(_)` rather than a separate variant — is fully implemented and load-bearing for downstream walkers.

### M-002: Parser produces `body: Some(_)` for `sub name : Type { body }` form

- **State:** FICTION
- **Failure mode:** F1 (compile-time contract assumed by spec/PRD → no parser backing)
- **Evidence:** `tree-sitter-reify/grammar.js:470-494` only defines instantiation form (`sub name = StructName<...>(...)`) and collection form (`sub name : List<StructName>`); no `sub name : Type { body }` alternative. `crates/reify-syntax/src/ts_parser.rs:1705-1707` explicitly comments "Grammar does not yet produce specialization-scope bodies" and hardcodes `body: None`. Regression tests `sub_decl_specialization_tests.rs:4-17` document the intent that `body: None` is the only parser outcome today. No fused-memory task tracks the deferred grammar work.
- **Blocks:** every acceptance criterion (1-7) of this PRD — none are reachable from `.ri` source. Compile-pipeline integration test `specialization_scope_validation_tests.rs:78-98` hand-builds AST nodes to drive the validator because tree-sitter cannot.
- **Note:** The validator, diagnostic, walker, and LSP plumbing all exist and pass tests — but every test is AST-builder-only. Real users writing `sub motor : ElectricMotor { thickness = 3mm }` get either a parse error (if `: Type {` is unrecognized) or silent acceptance via a different path — neither matches the PRD contract. This is the structural-ctor / GR-001 shape: PRD assumes runtime/end-to-end mechanism, implementation provides only the back half.

### M-003: Single-pass validator over spec-scope members

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/compile_builder/specialization_scope_check.rs:30-54` (`validate_module`); also delegates to `reify_syntax::walk_specialization_scope_members` (`lib.rs:305-312`)
- **Blocks:** none
- **Note:** Mirrors `dot_chain_lint` / `shadow_lint` signature; receives the same `&mut Vec<Diagnostic>` plumbing. Tested via dedicated unit tests in the same file (10+ tests).

### M-004: `param`-in-spec-scope rejection

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `specialization_scope_check.rs:73-74` (`forbidden_decl_info` arm); test `validate_module_emits_forbidden_decl_diagnostic_for_param_inside_specialization_scope` at lines 480-530
- **Blocks:** none
- **Note:** Acceptance criterion 1. Reachable only via AST-builder tests (see M-002).

### M-005: `port`-in-spec-scope rejection

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `specialization_scope_check.rs:75` (`forbidden_decl_info` arm); test `validate_module_emits_forbidden_decl_diagnostic_for_port_inside_specialization_scope` at lines 439-472
- **Blocks:** none
- **Note:** Acceptance criterion 2. Reachable only via AST-builder tests.

### M-006: `sub`-in-spec-scope rejection

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `specialization_scope_check.rs:76` (`forbidden_decl_info` arm); test `validate_module_emits_forbidden_decl_diagnostic_for_bare_sub_inside_specialization_scope` at lines 397-430
- **Blocks:** none
- **Note:** Acceptance criterion 3. Reachable only via AST-builder tests.

### M-007: `DiagnosticCode::SpecializationForbiddenDecl` (E_SPECIALIZATION_FORBIDDEN_DECL)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-types/src/diagnostics.rs:477-494` (enum variant + docstring with mnemonic); round-trip tests at lines 1319-1344 (`code_round_trip`, serde PascalCase pinning)
- **Blocks:** none
- **Note:** PRD-prose mnemonic `E_SPECIALIZATION_FORBIDDEN_DECL` documented in the variant docstring; canonical message form pinned in compiler-side unit tests.

### M-008: Permitted forms (assignments, `constraint`, `let`, `connect`, `where`) leave validator silent

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `specialization_scope_check.rs:77-84` (LOAD-BEARING wildcard arm + docstring); test `validate_module_emits_no_diagnostic_for_permitted_decls_inside_specialization_scope` at lines 374-386 (uses `let` + `constraint` fixtures)
- **Blocks:** none
- **Note:** The wildcard arm is intentionally `None`; a future `MemberDecl` variant won't silently become forbidden. The test guards against accidental broadening. PRD acceptance criterion 4 (and partially 5) covered.

### M-009: Nested specialization-scope traversal (parent-before-children, one diagnostic per forbidden decl)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `reify_syntax::walk_members_depth` (`lib.rs:314-352`) recurses into `MemberDecl::Sub(s).body`; test `validate_module_emits_diagnostic_for_each_forbidden_decl_in_nested_specialization_scope` at `specialization_scope_check.rs:302-362`
- **Blocks:** none
- **Note:** PRD clause "Applies anywhere a specialization scope appears: top-level `sub` blocks, nested `sub` blocks ...". Bounded by `MAX_MEMBER_NESTING_DEPTH = 32` (`lib.rs:267`) for fuzzer safety.

### M-010: `match`-arm `sub` blocks as specialization scopes (per-arm body recursion)

- **State:** PARTIAL
- **Failure mode:** F1 (validator wired; producer absent)
- **Evidence:** Walker handles `MemberDecl::MatchArmDeclGroup` arms at `reify-syntax/src/lib.rs:344-347` (task 2372 ref); compile-pipeline integration test `crates/reify-compiler/tests/specialization_scope_validation_tests.rs:128-175` exercises a hand-built `MatchArmDeclGroup` nested in an outer `sub`-with-body. BUT: no grammar rule for `match_arm_decl_group` (grep on `tree-sitter-reify/grammar.js` is empty); cross-check `crates/reify-syntax/tests/boundary1_producer.rs:553` ("Not produced by the tree-sitter parser yet (task 2372)").
- **Blocks:** PRD scope clause "match-arm sub blocks" — not reachable from `.ri` source until task 2372's grammar half lands (status of that grammar work is not visible in code as a TODO/task ID under specialization-scope).
- **Note:** Symmetric with M-002: validator/walker exist; parser does not produce the input variant. Cross-breadcrumb to `match-block-decls` PRD audit (task 2372 territory).

### M-011: `forall ... : connect`/`constraint` desugarings that "generate specialization scopes"

- **State:** WIRED (vacuously) — the conditional clause has no current trigger
- **Failure mode:** N/A
- **Evidence:** `tree-sitter-reify/grammar.js:676-688` restricts `forall_statement` body to `connect_statement | chain_statement | constraint_declaration | constraint_instantiation`; none of those produce a `SubDecl` with `body: Some(_)`. AST level: `ForallConnectDecl` / `ForallConstraintDecl` (`reify-syntax/src/lib.rs:461-479`) carry connect/constraint bodies — never sub bodies.
- **Blocks:** none
- **Note:** The PRD says "**if** they generate specialization scopes". As of today they do not. If a future `forall ... : sub ...` form is added, the walker entry point in `for_each_specialization_member` (`specialization_scope_check.rs:100-128`) would need to be extended (its current Declaration::Function/Field/Constraint/Enum/Unit/TypeAlias/Import arms `continue`, and there is no Forall declaration kind on the Declaration enum to handle — forall is a MemberDecl, not a top-level Declaration). The exhaustive-match guard (`lib.rs:108-124` comment) only fires when adding new `Declaration` variants, not new `MemberDecl` variants — so a future Forall-with-sub-body would silently miss this scope unless `walk_members_depth` is extended.

### M-012: Diagnostic span points at the forbidden keyword + name, not the whole body

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `specialization_scope_check.rs:73-76` returns each decl's own span (`p.span` / `s.span`); per-kind tests pin `d.labels[0].span == <decl>_span` (e.g. lines 522-525, 467-471, 426-429)
- **Blocks:** none
- **Note:** Acceptance criterion 6. The label message is the canonical `"forbidden in specialization scope"` (line 50). PRD asks for keyword+name span; the implementation provides the full decl span which contains both — close enough by current convention but a stricter "keyword+name only" span is not implemented.

### M-013: No parse-error / panic surfaces when forbidden form is attempted (graceful post-parse handling)

- **State:** WIRED (modulo M-002)
- **Failure mode:** N/A
- **Evidence:** Validator runs in `compile_with_prelude_context` after `forward_parse_errors` (`crates/reify-compiler/src/lib.rs:252-259`); collects diagnostics non-fatally; integration test `compile_pipeline_invokes_specialization_scope_validator` (`specialization_scope_validation_tests.rs:78-98`) confirms surfacing through `CompiledModule::diagnostics` rather than panicking.
- **Blocks:** Reachability of acceptance criterion 7 depends on M-002 — once the grammar admits the form, parse-error/panic absence is already guaranteed by the post-parse positioning.
- **Note:** Designed correctly as a post-parse pre-pass.

### M-014: LSP diagnostics channel surfaces `SpecializationForbiddenDecl`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-lsp/src/diagnostics.rs:1701-1820+` ("specialization-scope LSP regression locks (task 2371)"); diagnostics are translated to LSP via `convert::convert_diagnostic`; tests filter on `code == "SpecializationForbiddenDecl"`.
- **Blocks:** none
- **Note:** Task 2371 done/merged (commit 8cbc152b62). Reachable today only via the hand-built ParsedModule fixtures used in the LSP tests, again due to M-002.

### M-015: Compile pipeline wires the validator (single call site, uniform with other pre-passes)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/lib.rs:256-259` (single call in `compile_with_prelude_context`, the orchestrator shared by `compile_with_prelude_refs` and `compile_with_stdlib`); pipeline smoke test `compile_pipeline_invokes_specialization_scope_validator` at `specialization_scope_validation_tests.rs:78-98`
- **Blocks:** none
- **Note:** Sits between `shadow_lint` and the units phase; gets `&mut compile_ctx.diagnostics` per the lint signature convention.

## Cross-PRD breadcrumbs

- **match-block-decls PRD** owns the grammar/producer side of `MatchArmDeclGroup` (task 2372). The walker integration with specialization-scope (`walk_members_depth` arm at `reify-syntax/src/lib.rs:344-347`) lives in this PRD's downstream. If match-block-decls lands its grammar half, M-010's PARTIAL state would become WIRED automatically.
- **forall-statement-form PRD** owns the grammar/producer side of `forall` (grammar.js:676-688). If a future expansion adds `forall ... : sub <body>` it would interact with M-011 — the specialization-scope walker would need extension to traverse forall bodies that contain sub-with-body. Today's grammar restricts forall body to four non-sub kinds, so no gap.
- **Pattern parallel to GR-001 (struct-ctor runtime eval):** M-002 and M-010 are structurally identical to GR-001 — the consumer half (validator/walker/diagnostic/LSP) is fully built and tested, but the producer half (parser admitting the form) is silently absent with no task ID attached. The same "independent architects accreting decisions" failure mode the audit-brief describes.
