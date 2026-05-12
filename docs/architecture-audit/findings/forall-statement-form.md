# Audit: `forall` Statement Form (Per-Element `connect` / `constraint` Generation)

**PRD path:** `docs/prds/forall-statement-form.md`
**Auditor:** audit-forall-statement-form
**Date:** 2026-05-12
**Mechanism count:** 14
**Gap count:** 4

## Top concerns

- **Spec/PRD/code three-way drift on `chain`.** PRD §"Out of scope" enumerates `for` loops and `forall (a,b)` but says nothing about `chain`. Spec §5.4 says v0.1 statement scope is "`connect` and `constraint` only". Yet the grammar (`tree-sitter-reify/grammar.js:676-688`), AST (`reify_syntax::ForallConnectBody::Chain`), compiler (`forall_elaborate.rs:781-838`), and corpus tests all support `forall v in c: chain ...`. This is real shipping behaviour that the PRD does not authorise — Phase 3 must reconcile.
- **PRD criterion 9 (guard composition) is only half-wired.** Per-decl body `where`-clauses on `forall ... : constraint X where W` do compose into per-element `CompiledGuardedGroup`s (tested by `forall_constraint_with_body_where_clause_emits_per_element_guarded_groups`). However an **outer** structure-level `where`-guarded block containing a `forall` statement is rejected with an error diagnostic at `guards.rs:544-559` — "forall connect/chain statements in guarded blocks are not yet supported". PRD §"Guard interaction" and Scope describe outer-guard inheritance ("composes conjunctively with each generated decl's guard, mirroring `where`-block desugaring"). The stub test `forall_connect_inside_guarded_block_emits_stub_error` (forall_statement_stub_tests.rs:34) pins the rejection. Spec §5.4 closing paragraph is treated by code as "out of scope for v0.1" with no follow-up task on file.
- **PRD criterion 7's `undef`-count deferred path is feature-flagged by body shape.** Constraint-arm (task 2629) and Connect-arm (task 2690) re-elaborate at runtime, but `ConstraintInst`, `Chain`, where-clause-bearing bodies, rich-form connectors (`via T(args)`), and unsupported port shapes ALL emit info diagnostics and silently skip runtime capture (forall_elaborate.rs:312-325, 362-370, 642-656, 700-705; resolve_port_name limitation at 598-625). PRD criterion 7 reads as unconditional; the reality is a thicket of partials.
- **Forall in purpose bodies is rejected** (`traits.rs:448-471`). Not mentioned in PRD, not pinned by a follow-up task in our metadata, but a foreseeable user expectation that fails silently from a PRD-reader's perspective.

## Mechanisms

### M-001: Grammar disambiguation by post-`:` token (statement vs expression form)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `tree-sitter-reify/grammar.js:676-688` (`forall_statement` reachable only via `_member`, not `_expression`; body is `choice($.connect_statement, $.chain_statement, $.constraint_declaration, $.constraint_instantiation)`); `tree-sitter-reify/test/corpus/forall_statement.txt:52-77` ("expression-form unchanged (regression)") + lines 199-230 ("nested-quantifier collection (regression pin)"); rationale comment lines 666-675.
- **Blocks:** none
- **Note:** Disambiguation is achieved by scoping `forall_statement` to member-context only; expression-form `quantifier_expression` lives in expression-context. GLR resolves cleanly because the statement body must begin with `connect`/`chain`/`constraint`.

### M-002: AST variants `ForallConnect` / `ForallConstraint` with body sub-enums

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-syntax/src/lib.rs:93-95,465-507` (`MemberDecl::ForallConnect(ForallConnectDecl)` / `MemberDecl::ForallConstraint(ForallConstraintDecl)`, `ForallConnectBody { Connect, Chain }`, `ForallConstraintBody { Constraint, Instantiation }`); `ts_parser.rs:2023-2090` (tree-sitter → AST lowering).
- **Blocks:** none
- **Note:** Body shape carries 4 logical variants total (Connect, Chain, Constraint, Instantiation). PRD only commits to Connect + Constraint; Chain + Instantiation are real bonus shapes (see M-013).

### M-003: Deferred elaboration sub-pass after sub_components/value_cells populated

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/entity.rs:1655-1666` (push into `pending_forall_connect` / `pending_forall_constraint`); `entity.rs:1692-...` ("Deferred forall elaboration sub-pass (task 2364, spec §5.4)" comment); `forall_elaborate.rs:9-13` (entry-point docstring confirms post-main-loop dispatch).
- **Blocks:** none
- **Note:** Two-phase compilation needed because element count comes from `__count_<name>` value cells that are not populated until after the main member loop.

### M-004: Per-element substitution + emission (literal-collection or known-count sub)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `forall_elaborate.rs:115-152` (literal & count-resolved arms of `resolve_forall_elements`); 377-487 (per-element emission for Constraint/Instantiation); 729-841 (Connect/Chain); compile-time tests `forall_constraint_over_list_literal_emits_per_element_constraints`, `forall_constraint_over_collection_sub_with_known_count_emits_per_element_constraints`, `forall_connect_emits_per_element_connections`, `forall_constraint_inst_body_emits_per_element_inst_predicates`.
- **Blocks:** none
- **Note:** Acceptance criteria 1, 2, 5, 8 fully covered for the resolved case.

### M-005: Element-index span / label `forall@<var>[<idx>]` plumbed to LSP

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** Labels emitted at `forall_elaborate.rs:390` (constraint) and `engine_edit.rs:1819,1881` (runtime arms); LSP test `forall_per_element_constraint_violation_surfaces_element_index` at `reify-lsp/src/diagnostics.rs:1010-1098` confirms the label survives end-to-end and per-index diagnostic enumeration (PRD criterion 10).
- **Blocks:** none
- **Note:** Span anchors at the source `forall` decl; index encoded in label suffix.

### M-006: Empty-collection: zero decls, no error (PRD criterion 6)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** Tests `forall_constraint_over_empty_list_literal_emits_no_decls_no_error`, `forall_constraint_over_zero_count_collection_sub_emits_no_decls_no_error`, `forall_connect_chain_body_over_empty_list_literal_emits_no_connections_no_diagnostic`; intentional-placement comments at `forall_elaborate.rs:438-448` and 789-797 explaining the loop-body placement that pins criterion 6.
- **Blocks:** none
- **Note:** Loop placement is intentional and pinned by test; refactor risk well-documented.

### M-007: Undef-count deferral — Constraint body — runtime re-elaboration

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** Compile-time capture at `forall_elaborate.rs:330-360` builds `CompiledForallTemplate { body: CompiledForallBody::Constraint }`; runtime emission at `engine_edit.rs:1843-1900`; threaded via `EvaluationGraph::forall_templates` (`graph.rs:182,236`) and per-template ledger `Snapshot::forall_emitted` (`snapshot.rs:27,69`); test `edit_param_count_undef_to_known_emits_per_element_forall_constraints` end-to-end; tasks 2629 (done, commit `1dd77a8bfa`) + 2364 (done).
- **Blocks:** none
- **Note:** Drain-on-decrease, cache invalidation per emitted ConstraintNodeId, fingerprint stability all covered by `forall_runtime_re_elaboration.rs` suite.

### M-008: Undef-count deferral — Connect body — runtime re-elaboration

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** Compile-time capture at `forall_elaborate.rs:554-697`; runtime emission at `engine_edit.rs:1756-1841`; test `edit_param_count_undef_to_known_emits_per_element_forall_connections`; task 2690 (done, commit `39da6b1791`).
- **Blocks:** none
- **Note:** Distinct `forall_connect:` cnid namespace from Constraint-arm `forall:` to avoid drain-collisions; synthetic compatibility constraint emits `Bool::True` literal — direction-check and `connector_sub`/`frame_constraint` auto-creation explicitly out of task scope (see M-011).

### M-009: Body `where` clause composes conjunctively into per-element `CompiledGuardedGroup`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `forall_elaborate.rs:404-425` (Constraint body's `where` routed through `compile_per_decl_constraint_guard`); 449-486 (ConstraintInst body's `where`); tests `forall_constraint_with_body_where_clause_emits_per_element_guarded_groups` (constant guard `where heavy`) and `forall_constraint_body_where_clause_referencing_bound_var_substitutes_per_element` (per-element substituted guard `where v.mass > threshold`).
- **Blocks:** none
- **Note:** Covers HALF of PRD criterion 9 — the per-element body `where`. The OUTER structure-level guard composition is M-012 (NOT wired).

### M-010: Non-iterable diagnostic ("cannot iterate over non-collection type")

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `forall_elaborate.rs:188-214` (`diagnose_non_iterable_or_skip` w/ anti-cascade for Type::Error and silent-defer for List/Set with deferred count); test `forall_over_non_iterable_collection_emits_diagnostic`.
- **Blocks:** none
- **Note:** Wording mirrors the expression-form forall/exists site (cited at `expr.rs:1791-1799`) for symmetry.

### M-011: Connector-spec (`via T(args)`) propagation on deferred-count Connect

- **State:** PARTIAL
- **Failure mode:** TODO (info-diagnostic stub, future-task tagged but no task ID filed under PRD)
- **Evidence:** `forall_elaborate.rs:642-656` — when `cd.connector_type.is_some() || !cd.params.is_empty()` and count is deferred, emits info "connector type and params are not propagated by runtime re-elaboration; only the port-to-port connection is materialised (task 2690 future scope)"; runtime path drops the captured `connector_type` and `params` (`engine_edit.rs:1760-1763` destructures with `_`); test `forall_connect_rich_form_over_undef_count_collection_sub_emits_connector_drop_info_diagnostic`.
- **Blocks:** none filed
- **Note:** Resolved-count path DOES propagate connectors via `compile_connection`. Asymmetry between resolved and deferred paths is unflagged in PRD; no follow-up task in metadata. Documented partial.

### M-012: Outer structure-level `where`-block containing `forall` statement

- **State:** TODO
- **Failure mode:** Hard error returned today; PRD scope explicitly includes this
- **Evidence:** `crates/reify-compiler/src/guards.rs:544-559` ("forall connect/chain statements in guarded blocks are not yet supported" / "forall constraint statements in guarded blocks are not yet supported"); stub test `forall_connect_inside_guarded_block_emits_stub_error` at `forall_statement_stub_tests.rs:34`; PRD Scope ("Guard interaction: the surrounding scope's `where` guard composes conjunctively with each generated decl's guard").
- **Blocks:** PRD criterion 9 (partial), spec §5.4 closing paragraph compliance.
- **Note:** No tracking task surfaced in metadata for this specifically — the existing stub error is shipped as final v0.1 behaviour. Spec says "When the guard is inactive, the quantifier is absent from the evaluation graph entirely"; today the compiler instead errors-out at compile time on the source form, never reaching runtime guard evaluation.

### M-013: PRD-out-of-scope body shapes shipped as functional

- **State:** ORPHAN (re: PRD; the PRD has no use case but the mechanism is shipping)
- **Failure mode:** DRIFT vs spec §5.4 "v0.1 statement scope: `connect` and `constraint` only"
- **Evidence:** Grammar `forall_statement` accepts `chain_statement` and `constraint_instantiation` bodies (`grammar.js:682-687`); AST variants `ForallConnectBody::Chain` and `ForallConstraintBody::Instantiation` (`reify-syntax/src/lib.rs:478-507`); compiler arms at `forall_elaborate.rs:430-486` (Instantiation), 781-838 (Chain); corpus tests `forall_statement.txt` lines 147-197 (Instantiation, Chain); compile-time test `forall_connect_chain_body_emits_per_element_pairwise_connections`.
- **Blocks:** none
- **Note:** This PRD does not authorise `chain` or `constraint_instantiation` as `forall` body shapes, yet both work. Spec §5.4 explicitly restricts v0.1 to "`connect` and `constraint` only". Phase 3 must decide: (a) widen PRD/spec to legitimise, or (b) accept as DRIFT and document, or (c) deprecate. The Chain-arm partial in M-014 only fires for the deferred path; resolved-count Chain ships fine.

### M-014: Undef-count deferral — `ConstraintInst` / `Chain` / where-clause / port-shape bodies

- **State:** PARTIAL
- **Failure mode:** Multiple info-diagnostic stubs labelled "task 2629 future scope" / "task 2690 future scope" / "task 2717 …" with no umbrella follow-up
- **Evidence:** `forall_elaborate.rs:362-370` (Instantiation body deferred — info diag); 312-325 (Constraint body with where-clause + deferred — info diag); 699-705 (Chain body deferred — info diag); 600-625 (Connect with unsupported port shape — info diag, task 2717 done); tests `forall_constraint_inst_body_over_undef_count_collection_sub_skips_capture_with_info_diagnostic`, `forall_chain_over_undef_count_collection_sub_skips_capture_with_info_diagnostic`, `forall_constraint_with_where_clause_over_undef_count_collection_sub_skips_capture_with_info_diagnostic`.
- **Blocks:** PRD criterion 7's universal coverage claim (partial only).
- **Note:** All these cases compile silently to "zero decls, no error" at first eval, then never re-emit when count becomes known. Behaviour is documented per-call-site via info diagnostics but the PRD does not surface these as known limitations. Task 2717 was filed and is now done — that one specifically catches port-shape mismatches with a label. The other three cases are not tracked by per-mechanism tasks visible in our metadata.

## Cross-PRD breadcrumbs

- **`chain` statement form** appears in spec §5.4 indirectly (as a member-level statement that can appear inside `forall`); it likely has its own non-forall PRD or grew in spec without a PRD. Not investigating — out of scope.
- **`@forall` over purpose bodies** is currently rejected (`traits.rs:448-471`). If a future PRD for purposes wants per-element purpose-body forall, it will need to coordinate with this PRD.
- **`SchemaNode` runtime re-elaboration** referenced by PRD §"Determinacy" was never built as a `Rust struct` — instead `engine_edit.rs`'s count-cell phase grew the re-emission logic in-place. If a separate "SchemaNode" PRD lands, watch for shape collision with `forall_templates` + `forall_emitted` plumbing already on `EvaluationGraph` / `Snapshot`.
- **`connector_sub` auto-creation + `frame_constraint` generation** are out of scope here but are owned by the connect PRD (or equivalent). The runtime-deferred Connect arm explicitly drops both — see M-011.
