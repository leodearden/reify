# PRD: `forall` Statement Form (Per-Element `connect` / `constraint` Generation)

> **2026-05-12 grammar-fiction sweep** (docs/architecture-audit/phase-3-grammar-fiction-triage-log.md):
> Originally the audit flagged `chain` body as a grammar fiction; in fact
> the inverse holds — grammar (`tree-sitter-reify/grammar.js:684`), AST
> (`ForallConnectBody::Chain`), and compiler (`forall_elaborate.rs:781-838`)
> all ship `chain` and `constraint_instantiation` as valid `forall`
> statement-form bodies. This PRD originally limited statement scope to
> `connect`/`constraint` per spec §5.4; updated below to legitimize the
> shipped widening. See findings/forall-statement-form.md M-013.

## Goal

Support the **statement form** of `forall` for generating per-element
`connect`, `chain`, or `constraint` declarations. The original v0.1 scope
named `connect` and `constraint` per spec §5.4; the implementation also
supports `chain` body (which desugars to pairwise `connect` semantics) and
constraint-instantiation body, both shipped and tested. This is distinct
from the **expression form** (`forall v in vents: v.spacing > 10mm`
returning `Bool`) which already exists (#63 done).

## Background

- Spec §5.4 (lines 1035-1057):
  - Expression form: `forall v in vents: v.spacing > 10mm` — produces `Bool` (existing, #63).
  - Statement form: `forall v in vents: connect v.inlet -> housing.air_channel` and `forall v in vents: constraint v.mass < 50g` — generates per-element decls.
  - Disambiguation: by the **token immediately after the colon**. `connect` or `constraint` → statement; anything else → expression.
  - v0.1 statement scope: `connect` and `constraint` only.
  - Empty-collection rules (vacuous truth/falsity for expression form) apply trivially to statement form: zero declarations generated.
  - Guarded collections: a `forall` statement over a guarded collection inherits the guard; per §5.4 closing paragraph, when the guard is inactive the quantifier is absent from the evaluation graph entirely.

## Scope

- Parser: extend the `forall` rule so the body alternative dispatches on the token after `:`. Today `forall ... : <expr>` only. New: also accept `forall ... : connect <connect-stmt>` and `forall ... : constraint <constraint-body>`.
- AST: introduce statement-form variants (`ForallConnect`, `ForallConstraint`) so the compiler distinguishes them from the expression form without loss.
- Compiler lowering: walk the statement-form node and emit one connect / constraint declaration per element of the collection (when the collection is structurally determined). Diagnostics are preserved per element — each generated decl carries a span back to the source `forall` plus the element index, so per-element constraint failures report the offending index/element.
- Determinacy:
  - If the collection structure (count) is determined, generate decls during elaboration.
  - If the collection structure is `undef`, the statement-form `forall` does not yet contribute decls — the SchemaNode re-elaborates once count is known (consistent with §6.2 "Collection size change" topology trigger).
- Guard interaction: the surrounding scope's `where` guard composes conjunctively with each generated decl's guard, mirroring `where`-block desugaring (§6.3).

## Out of scope

- `let`, `sub`, `param`, `port` as statement-form bodies (v0.1 explicitly limits to `connect` and `constraint`).
- `for` loops or other imperative iteration.
- `exists` statement form (no use case in the spec).
- `forall` over multiple bound vars (`forall (a, b) in pairs: ...`) — not in v0.1 spec.

## Acceptance criteria

1. Parser: `forall v in vents: connect v.inlet -> housing.air_channel` parses to a `ForallConnect` AST node.
2. Parser: `forall v in vents: constraint v.mass < 50g` parses to a `ForallConstraint` node.
3. Parser: `forall v in vents: v.spacing > 10mm` continues to parse as the expression form (no regression to #63).
4. Disambiguation rule: the token immediately following `:` is what selects the form. `connect` or `constraint` → statement; otherwise expression. Document and test.
5. Lowering: a structure with `sub vents : List<Vent>` of count 3 plus `forall v in vents: constraint v.mass < 50g` produces 3 distinct ConstraintNodes in the evaluation graph, each with span info pointing back to the `forall`.
6. Empty collection: zero generated decls; no error.
7. Undef-count collection: defers — generates 0 decls until count is determined; re-elaborates per §6.2 when count is set.
8. Connect form: `forall v in vents: connect v.inlet -> housing.air_channel` generates per-element `connect` artifacts (port-compat checks, frame alignment, etc., per §6.7).
9. Guard composition: `forall v in vents: constraint v.mass < 50g where heavy_vents` composes guards correctly per spec §6.3.
10. Diagnostic on per-element failure cites the element index in the source iteration.

## Task breakdown

1. Grammar / tree-sitter update: add `forall_statement_form` alternative; verify expression form still preferred when next token after `:` is anything but `connect`/`constraint`.
2. AST + compiler IR: `ForallConnect`, `ForallConstraint` node kinds.
3. Elaboration: in SchemaNode.compute(), iterate over the bound collection (when structurally determined) and emit per-element decls, threading element-index span info.
4. Empty-collection and undef-count handling per acceptance criteria 6/7.
5. Tests: parser tests (statement vs expression disambiguation), lowering tests (3-element constraint), connect-form lowering test, guard composition test, undef-count deferral test.
6. LSP / diagnostic test: per-element constraint violation reports element index.
