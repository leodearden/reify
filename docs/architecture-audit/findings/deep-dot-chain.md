# Audit: Deep Dot-Chain Warning

**PRD path:** `docs/prds/deep-dot-chain.md`
**Auditor:** audit-deep-dot-chain
**Date:** 2026-05-12
**Mechanism count:** 13
**Gap count:** 3

## Top concerns

- **Threshold configurability is FICTION.** PRD §"Scope" says "Threshold configurable but with a hardcoded v0.1 default of 4". The implementation hard-codes `pub(crate) const DEEP_DOT_CHAIN_THRESHOLD: usize = 4` at `crates/reify-compiler/src/compile_builder/dot_chain_lint.rs:37`. There is no settings/pragma/env path that overrides it. PRD §"Out of scope" says "Per-project threshold override (config knob can come post-v0.1)" — internally consistent if read as "knob is post-v0.1", but the §"Scope" wording is contradicted. Phase 3 should decide whether to amend the PRD to drop "configurable but" or file a follow-up.
- **Method-call out-of-scope is satisfied vacuously, not deliberately.** PRD §"Out of scope" and acceptance criterion #3 carve out method-call chains (`a.b.foo().c.d`). But Reify has no method-call AST shape: `ExprKind::FunctionCall { name: String, args: Vec<Expr> }` (`crates/reify-syntax/src/lib.rs:844`) takes a bare name, not a callee `Expr`. Therefore `a.b.foo()` is not even parseable as a method call in Reify v0.1. The lint passes acceptance #3 automatically — but no test exercises it because the source `a.b.foo().c.d` cannot be expressed. This is a DRIFT-shaped issue: PRD assumes a language feature that isn't there.
- **Diagnostic-code wire string is `"DeepDotChain"`, not `"W_DEEP_DOT_CHAIN"`.** PRD §"Scope" suggests `W_DEEP_DOT_CHAIN` as an example wire code (parenthetical "e.g."). Implementation surfaces `DiagnosticCode::DeepDotChain`, converted to LSP `code = "DeepDotChain"` via serde (`crates/reify-lsp/src/convert.rs:187-194` + `:424`). DRIFT, low severity — the PRD hedge "e.g." softens the contract, but downstream consumers grepping for `W_DEEP_DOT_CHAIN` will find nothing.
- **Walker coverage is unusually thorough.** Per-position regression tests (27 positions) and an `ArmKind::*` table-driven depth-guard test (`MAX_EXPR_DEPTH = 256`) lock down every expression-bearing slot. This PRD is one of the better-WIRED ones in the corpus.

## Mechanisms

### M-001: AST `MemberAccess` chain-depth counter (single-pass, syntactic)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/compile_builder/dot_chain_lint.rs:343-376` (iterative `while let ExprKind::MemberAccess { object, member }` walk); chain length computed as `1 + members_outer_to_inner.len()`.
- **Note:** Counting matches PRD §"Acceptance criteria" #1/#2: `a.b.c.d` (len 4) does not warn; `a.b.c.d.e` (len 5) warns. Pinned by `tests/deep_dot_chain_tests.rs::chain_at_threshold_does_not_warn` + `::chain_above_threshold_emits_one_warning_with_deep_dot_chain_code`.

### M-002: Threshold gate `chain_len > DEEP_DOT_CHAIN_THRESHOLD`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `dot_chain_lint.rs:362` (strict `>`, not `>=`).
- **Note:** Boundary semantics match PRD: at-threshold OK, one-over warns. Regression-locked by `chain_at_threshold_does_not_warn` (also a positive control: a length-6 chain in the same source must warn, so a disabled pass fails the assertion).

### M-003: Threshold configurability (runtime override path)

- **State:** FICTION
- **Failure mode:** F1 (PRD contract → no runtime backing)
- **Evidence:** `pub(crate) const DEEP_DOT_CHAIN_THRESHOLD: usize = 4` at `dot_chain_lint.rs:37`. No knob in `reify-compiler` Cargo features, no project-config, no pragma, no env var. `grep -n "config\|configurable\|override\|env\|pragma" crates/reify-compiler/src/compile_builder/dot_chain_lint.rs` yields zero matches.
- **Blocks:** none (PRD §"Out of scope" defers the project-level override post-v0.1)
- **Note:** PRD §"Scope" wording "Threshold configurable but with a hardcoded v0.1 default of 4" is internally inconsistent with §"Out of scope" listing "Per-project threshold override (config knob can come post-v0.1)". Implementation tracks the latter. Low priority for v0.1; Phase 3 may want a follow-up task to make the const `pub` + accept a `&CompilerConfig` arg before any user-visible knob.

### M-004: New diagnostic code `DiagnosticCode::DeepDotChain`

- **State:** WIRED (with DRIFT on the wire string vs the PRD example)
- **Failure mode:** F1 (minor — example wire code differs from chosen wire code)
- **Evidence:** Variant declared at `crates/reify-types/src/diagnostics.rs:308`; equality/Debug pinned at `:1179-1188`; LSP wire serialization via serde at `crates/reify-lsp/src/convert.rs:187-194`; PascalCase-stability lock at `:420-424` `(DiagnosticCode::DeepDotChain, "DeepDotChain")`.
- **Note:** Wire string is `"DeepDotChain"`, not the PRD's parenthetical `W_DEEP_DOT_CHAIN`. PRD prefix "e.g." is permissive; flag as DRIFT for Phase 3 only because external tooling that grep'd the PRD literally would miss the lint.

### M-005: Diagnostic message format with chain text + depth

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `dot_chain_lint.rs:365-368` — `"deep dot-chain (depth {chain_len}): {chain_text} — consider intermediate let-bindings"`. Render helper `render_chain_text` at `:487-502`. Test `chain_warning_message_contains_full_chain_text` at `tests/deep_dot_chain_tests.rs:113-133`.
- **Note:** PRD acceptance #2 ("full chain text and span") is met.

### M-006: Diagnostic span (`DiagnosticLabel`) covers full chain

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `dot_chain_lint.rs:370` — `.with_label(DiagnosticLabel::new(expr.span, "deep dot-chain"))` where `expr.span` is the outermost MemberAccess span. Test `chain_warning_has_label_covering_full_chain_span` at `tests/deep_dot_chain_tests.rs:138-177` asserts `label.span.start == start(a) && label.span.end == end(e)` for `a.b.c.d.e`.

### M-007: LSP path — DiagnosticCode → `lsp_types::Diagnostic` with code/severity/source

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-lsp/src/convert.rs:235-244` builds `lsp_types::Diagnostic { code: Some(NumberOrString::String("DeepDotChain")), severity: Some(WARNING), source: Some("reify"), ... }`. End-to-end test `lsp_compute_diagnostics_surfaces_deep_dot_chain_warning` at `crates/reify-lsp/src/diagnostics.rs:489-535`.
- **Note:** Test deliberately does NOT assert on the code field — comment at `:483-487` cites a plan-level decision "Do NOT modify convert_diagnostic to populate `lsp_types::Diagnostic.code`". The current code IS populated however (verified at `convert.rs:187-194`); the comment may be stale plan-language, not a code gap. Worth a quick Phase 3 sanity check.

### M-008: Method-call chain out-of-scope (`a.b.foo().c.d` does not warn)

- **State:** FICTION (PRD assumes language feature not present)
- **Failure mode:** F1 (PRD describes a shape the AST does not carry)
- **Evidence:** Reify `ExprKind::FunctionCall { name: String, args: Vec<Expr> }` at `crates/reify-syntax/src/lib.rs:844` — name is a bare `String`, not an `Expr` callee. No grammar rule for method-call (`grep -rn "method_call\|MethodCall" crates/tree-sitter-reify/grammar.js crates/reify-syntax/` is empty). `tests/deep_dot_chain_tests.rs` has no `foo()` method-call test; the closest is `function_call_root_emits_function_call_placeholder` which uses `f(a).a.b.c.d.e` (free-function call as a chain root).
- **Blocks:** none today (vacuously satisfied)
- **Note:** Acceptance criterion #3 (`a.b.foo().c.d` not tripping the lint) cannot be exercised because the source cannot be parsed. PRD authors likely assumed Reify had Rust-like method-call dot syntax. If method-call syntax is later added to Reify (e.g. as `FunctionCall { callee: Box<Expr>, ... }`), the lint will need a dedicated test and possibly a chain-walk extension. Flag this for Phase 3 cross-PRD breadcrumb tracking — other PRDs may also assume method-call syntax.

### M-009: Indexing breaks the chain (`a.b[0].c.d.e` counts hops post-index)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `dot_chain_lint.rs:408-416` `ExprKind::IndexAccess` arm — IndexAccess is a non-MemberAccess node, so the `while let` chain-counter terminates at it; recursion descends into both `object` and `index` children for nested chains. Test `index_access_resets_chain_root_emits_one_warning_post_index` at `tests/deep_dot_chain_tests.rs:186-232` exercises `a.b[0].c.d.e.f` (one warning, post-index chain length 5).
- **Note:** Behaviour matches PRD acceptance #4 verbatim.

### M-010: Walker coverage of every expression-bearing AST position

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `walk_declaration` (`dot_chain_lint.rs:85-157`) covers Structure/Occurrence/Trait/Purpose/Function/Field/Constraint/Unit/Enum/Import/TypeAlias. `walk_members` (`:183-285`) covers every `MemberDecl` variant including nested `GuardedGroup`/`Port`/`ForallConnect`/`ForallConstraint` bodies. Per-position regression tests at `tests/deep_dot_chain_tests.rs:346-773` cover 27 distinct slots (Positions 1-27, each named after the AST slot it exercises).
- **Note:** This is unusually thorough — most lints in the corpus have one happy-path test; this one has one test per AST slot. Walker omissions would now fail loudly and locally.

### M-011: Stack-safety bound on structural recursion (`MAX_EXPR_DEPTH = 256`)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `dot_chain_lint.rs:55` const; `walk_expr_depth` guard at `:322-340` with `debug_assert!(false, ...)` in debug + silent `return` in release. Table-driven test `walk_expr_depth_panics_for_every_recursion_arm` at `:535+` exercises every recursion arm.
- **Note:** Iterative `while let` for MemberAccess chain walk uses one frame regardless of N (`:351-358`); the structural-recursion guard only protects against deeply-nested non-MA expressions (Conditional/BinOp/Lambda/etc.). The chain-counting loop itself is unbounded by design — a fuzz-input chain of millions of `.field` hops would not overflow the stack but would heap-allocate a `members_outer_to_inner: Vec<&str>` of that length. Not a gap, but a worth-noting characteristic.

### M-012: Chain text rendering with shape-hinting root placeholders

- **State:** WIRED (ORPHAN-flavoured: PRD does not specify this)
- **Failure mode:** N/A
- **Evidence:** `render_chain_text` at `dot_chain_lint.rs:487-502` maps `IndexAccess → "_[…]"`, `FunctionCall → "_(…)"`, others → `"_"`. Tests `index_access_resets_chain_root_emits_one_warning_post_index` and `function_call_root_emits_function_call_placeholder` lock the U+2026-ellipsis placeholders.
- **Note:** PRD says only "full chain text" without specifying how non-Ident chain roots should render. Implementation chose ellipsis-decorated placeholders; this is a useful design choice but is not driven by the PRD. Phase 3 may want to capture it in the PRD as part of acceptance #2 if the placeholder format is contract.

### M-013: Integration into compiler pipeline (`lint_module` invoked once per compile)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/lib.rs:254` — `compile_builder::dot_chain_lint::lint_module(parsed, &mut compile_ctx.diagnostics)` runs in `compile_with_prelude_context`, before shadow_lint and the specialization-scope check.
- **Note:** Runs unconditionally on every compile (no feature flag, no opt-out). LSP path picks it up via `compute_diagnostics` (verified by `lsp_compute_diagnostics_surfaces_deep_dot_chain_warning`).

## Cross-PRD breadcrumbs

- **Method-call syntax (M-008)** — PRD assumes `a.foo()` exists. Other PRDs may make the same assumption (e.g. any that describe fluent-API design). If/when method-call syntax is added to the AST, the dot-chain lint will need a dedicated test for acceptance criterion #3. Phase 3 should grep PRD corpus for `foo()` / `bar()` method-call patterns.
- **Diagnostic-code naming convention (M-004)** — PRD example `W_DEEP_DOT_CHAIN` doesn't match the codebase convention `DeepDotChain`. Other PRDs may use the `W_*` / `E_*` prefix style; worth a corpus-wide naming-drift pass.
- **Linter threshold configurability (M-003)** — Pattern: "configurable but post-v0.1". Several lints likely share this shape (shadow lint, specialization scope check). Worth seeing if the corpus would benefit from a shared `LintConfig` struct.
