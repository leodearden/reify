<!-- 2026-05-14 RECOVERY AUDIT TRAIL
This triage log was authored 2026-05-12. The (B) grammar-chain task IDs
referenced below were LOST in the 2026-05-13 fused-memory SIGABRT.
Full recovery in two passes:
  Pass 1 — worktree_orphans: B1 grammar+fixture 3475 → 3526.
  Pass 2 — agent re-file 2026-05-14:
    B1 lowering    3477 → 3558  [dep 3526]
    B1 e2e+LSP     3478 → 3559  [dep 3558]
    B2 grammar     3480 → 3563
    B2 lowering    3481 → 3564  [dep 3563]
    B2 e2e+LSP     3483 → 3567  [dep 3564]
    B3 grammar     3485 → 3569
    B3 lowering    3486 → 3571  [dep 3569]
    B3 e2e+LSP     3488 → 3573  [dep 3571]
The (A) PRD-prose rewrites and (C) skip cases are filesystem edits and
survived in git. Body preserved as historical record. See
docs/architecture-audit/gap-register.md top banner.
-->

# Phase 3 — Grammar-Fiction Triage Log

**Date:** 2026-05-12
**Scope:** 13 PRDs surfaced by cluster C-06 (`phase-3-files-synthesis.md` §1) as
assuming grammar productions that do not parse in `tree-sitter-reify/grammar.js`.
**Policy basis:**
- `feedback_prd_grammar_gate.md` (grammar/parser/lowering gate at PRD-decompose time)
- `feedback_task_chain_user_observable.md` (every leaf task names a user-observable signal)
- `preferences_implementation_chain_portfolio.md` (B + H portfolio; design-first for chains)
- `feedback_orchestrator_narrow_locks_favor_upfront_design.md` (queued tasks left
  `deferred`; broad cross-crate grammar+lowering work needs explicit activation)

**Method:** For each of the 13 PRDs, the auditor verified the grammar-fiction
claim against current `grammar.js` + ts_parser, then chose between:
- **(A)** Rewrite PRD prose — assumed grammar isn't worth shipping or can be
  expressed with existing grammar.
- **(B)** Ship the grammar — feature genuinely needs the new syntax; queue a
  grammar + lowering + user-observable-leaf chain.
- **(C)** Skip — audit mis-classified, grammar shipped between Phase 2 and now,
  or another PRD owns it (e.g. GR-001 follow-up PRD).

---

## (A) PRD prose rewrites

### A1 — `docs/prds/money-dimension.md`

**Surface retired.** `sum(buy.unit_cost * buy.quantity for buy in buys)` generator
comprehension — Reify has no comprehension AST node (audit M-014).

**Diff summary.**
- § Cost-aggregation `### Idiom` worked-example rewritten to use
  `[self.bolts.line_cost, self.mounts.line_cost].sum` — the actual idiom the
  canonical `examples/cost_aggregation.ri` uses (list-literal `.sum`).
- Filename hyphen → underscore (`cost-aggregation.ri` → `cost_aggregation.ri`)
  to match the on-disk reality.
- Added an `## Updates` lead-in at the top of the PRD recording the sweep.

**User-observable behavior.**
- *Retained:* aggregation of per-element `Money` values into a total cost.
- *Retired:* none — the implemented idiom was already what the audit found.

### A2 — `docs/prds/v0_3/multi-load-case-fea.md`

**Surfaces retired.** `subject to` clause on `minimize`, generator-comprehension
form `sum(... for ... in ...)`. The `= auto` value-default was **not** retired —
it is supported via `auto_keyword` at `grammar.js:430` (the distinct gap of
`auto:` in **type-arg** position is C-06's auto-resolution chain, filed under (B)).

**Diff summary.**
- `## Sketch of approach` user-pattern block: `subject to` → `where` (the
  spelling that `crates/reify-syntax/src/ts_parser.rs::lower_minimize_decl`
  actually parses).
- Task-7 of `## Decomposition plan` updated similarly; note added that the
  `auto` value-default keyword is grammar-supported.
- Added Updates note at the PRD top covering `subject to`, the unchanged
  `= auto` (value-default keyword, not type-arg), and the cross-link to the
  money-dimension comprehension rewrite.

**User-observable behavior.**
- *Retained:* the design-loop demo "minimize mass subject to envelope
  constraint" — expressible today as `minimize ... where ...`.
- *Retired:* none functionally; only the spelling.

**2026-05-27 amendment.** A2 correctly observed that the `= auto`
value-default was not retired from the multi-load-case-fea PRD because it
was already supported via `auto_keyword`. What A2 glossed was the *implicit*
scope question: the param-default position was always supported, but there
was no formal PRD covering the five binding-site positions (sub-instance
parameter overrides, named-argument values, let-binding RHS, structure
named-argument, connect-parameter assignment). A new PRD,
`docs/prds/auto-binding-site-positions.md`, now formalizes that broader
coverage and its α–ε task chain. Status as of 2026-05-27: α (task 3802,
commit e411301f69) is landed; β (task 3804) is in-progress; γ/δ/ε
(tasks 3805/3806/3807) are queued. The `= auto` fiction-flags that appeared
in C-06/GR-009 and the synthesis docs have been corrected in the same sweep
that filed this amendment.

### A3 — `docs/prds/v0_5/varying-thickness-shells.md`

**Surface retired (deferred to a separate grammar PRD).** `@shell(thickness = linear_taper(...))`
keyword-name-style annotation arg + function-call annotation arg + runtime-evaluable
annotation evaluation timing.

**Diff summary.**
- Added a top-of-file Updates note that calls out the three distinct grammar
  surfaces the syntax sketches assume, none of which exist today
  (`AnnotationArg` is a closed enum `{String|Int|Real|Bool|Ident}`;
  `@shell` accepts only positional Int/Real per `annotations.rs:154-176`).
- Strengthened `## Pre-conditions for activating` to include
  "Annotation-args expansion shipped" as an explicit prerequisite separate
  from the v0.4 shells foundation.
- Marked stdlib field-producer infrastructure as **not** "small additions" —
  they require typed `Field<Point3, Length>` producer fns.

**User-observable behavior.**
- *Retained:* the design intent of varying-thickness shells (per-vertex
  thickness preservation through extraction, kernel-side Gauss-point sampling,
  three user-specification modes).
- *Retired (effectively deferred):* the literal `@shell(thickness = linear_taper(...))`
  syntax — flagged as pending a separate annotation-args PRD with its own
  grammar chain. Activation of varying-thickness shells must now produce that
  PRD first.

### A4 — `docs/prds/v0_3/imported-field-source-hdf5-csv.md`

**Surfaces retired (deferred to a separate grammar PRD).** Extended
`imported`-block keys (`schema`, `dataset`, `axis_arrays`, `value_attribute`,
`units`, `interpolation`), the inline schema block `{ x: Length(mm), ... }`,
and the typed-column expression form `Length(mm)`.

**Diff summary.**
- Top-of-file Updates note enumerating the three distinct grammar gaps and
  noting that the PRD's claim "`Length(mm)` reuses the existing unit-literal
  grammar" is incorrect (`Length` is a dimension, not a callable; no
  type-applied-to-unit expression exists).
- Explicit guidance: do not queue any decomposition task referencing the new
  keys or the schema-block form until (i) the v0.2 OpenVDB glue produces a
  real `Value::Field` end-to-end (M-001 still pending despite tasks
  2665-2669 marked done — see audit), and (ii) a separate grammar PRD lands
  for the record-literal / typed-column / extended `imported`-block keys.

**User-observable behavior.**
- *Retained:* the v0.3 design intent (HDF5 + CSV ingestion).
- *Retired:* nothing operational ships today regardless.

### A5 — `docs/prds/v0_2/persistent-naming-v2.md`

**Surface retired.** "Absorbs the v0.1 `name = "..."` syntax" — this v0.1
syntax never existed in the parser (audit M-015).

**Diff summary.**
- "Replace, not augment." block rewritten: `user_label : Option<String>` is a
  **reserved runtime slot**, not an absorption of an existing parser form.
  Surface DSL for user-controlled face naming is explicitly named as a
  separate future grammar PRD with its own chain.
- Added Updates note at PRD top.

**User-observable behavior.**
- *Retained:* the runtime mechanism (selectors prefer user_label over
  (role, local_index) when both apply).
- *Retired:* nothing — the v0.1 `name = "..."` syntax never shipped, so no
  user-observable surface is removed.

### A6 — `docs/prds/shadowing-warning.md`

**Surface retired.** `#[allow(shadowing)]` Rust-bracket annotation form.
**Surface retired (scope reduction).** "Type aliases" as a shadowing-collision target.

**Diff summary.**
- Top-of-file Updates note re-spelling `#[allow(shadowing)]` →
  `@allow(shadowing)` to match the in-repo annotation framework
  (`@test`, `@optimized`, etc.), with a pointer to the actual file.
- `## Scope` list reduced — `type aliases` removed, since `TypeAlias` is only
  a top-level `Declaration` with no nested form (per M-017, no language
  position can shadow it).
- Lint-style paragraph updated to reflect the corrected suppression spelling.

**User-observable behavior.**
- *Retained:* the warning itself, its diagnostic shape, and the
  intentionally-conditional suppression mechanism.
- *Retired:* the Rust-bracket-style spelling; the "type aliases" target.

### A7 — `docs/prds/forall-statement-form.md`

**Surface legitimized (inverse case).** `chain` body in `forall` statement
form. The audit listed `chain` as a grammar fiction; in fact the grammar
(`grammar.js:684`), AST (`ForallConnectBody::Chain`), and compiler
(`forall_elaborate.rs:781-838`) all already ship `chain` body. The PRD
just hadn't authorised it (per spec §5.4 "v0.1 statement scope:
`connect` and `constraint` only").

**Diff summary.**
- Top-of-file Updates note recording that this is **inverse-fiction**: the
  feature exists, the PRD just didn't authorise it.
- `## Goal` updated to legitimize the three body shapes (`connect`, `chain`,
  `constraint`) that have shipped, citing audit M-013.

**User-observable behavior.**
- *Retained:* everything; this PRD-prose update only widens the documented
  scope to match shipped reality.

---

## (B) Grammar tasks filed

All filed via `submit_task(planning_mode=True)`, committed at status=`deferred`.
Per `feedback_orchestrator_narrow_locks_favor_upfront_design.md`, broad
cross-crate grammar + lowering chains should not auto-flip to `pending` —
Leo activates them explicitly when the corresponding PRD's value rises above
the broader corpus's narrow-file-lock work.

Each chain follows portfolio H (design-first) with task per (parser, lowering,
user-observable leaf) and explicit `dependencies` linking the chain.

### Chain B1 — `auto:` / `auto(free):` in type_arg_list

Covers **two** PRDs that share the same grammar surface:
- `docs/prds/auto-type-param-resolution.md` (v0.1 origin)
- `docs/prds/v0_2/auto-resolution-backtracking.md` (v0.2 extension)

| Task ID | Role | User-observable leaf signal |
|---|---|---|
| **3475** | Grammar production + parser fixture | `tree-sitter-reify parse` on a fixture file containing `Bearing<auto: Seal>` reports zero error nodes; `cargo test -p reify-syntax -- auto_type_arg_parses` passes. |
| **3477** | ts_parser lowering + compile-pipeline call-site | A `.ri` file with `Bearing<auto: Seal>` end-to-end emits a non-empty `auto_type_substitution`; ambiguous/no-candidate diagnostics surface through the standard channel. |
| **3478** | End-to-end fixture + LSP surface (leaf) | Determinism: two runs produce byte-identical resolved snapshots. LSP hover on the `auto:` site shows the resolved candidate. |

**Closes:** findings/auto-type-param-resolution.md M-009, M-010, M-016;
findings/auto-resolution-backtracking.md M-002, M-014.

### Chain B2 — Decl-level `match { ... => sub head : ... }` block

Covers `docs/prds/match-block-decls.md`.

| Task ID | Role | User-observable leaf signal |
|---|---|---|
| **3480** | Grammar production reachable from `_member` | `cargo test -p reify-syntax -- match_decl_block_parses_from_source` passes; tree-sitter parse on the fixture reports zero error nodes. |
| **3481** | `lower_match_arm_decl_group` in ts_parser | Existing hand-built AST integration tests can be rewritten to start from `.ri` source and continue to pass. |
| **3483** | End-to-end fixture + LSP surface (leaf) | `bolt.head` union typing resolves on hover; missing-field diagnostic names offending arms; variant-pipe arms (`Hex \| Button => sub head : RecessedHead`) elaborate cleanly. |

**Closes:** findings/match-block-decls.md M-001, M-003 (PRD AC 1, 2, 5, 8). AC 7
(reference safety from outside the match) remains deferred per M-013/M-014 —
that depends on a general guard-implication checker and is out of scope here.

### Chain B3 — `sub name : StructName { body }` specialization-scope body

Covers `docs/prds/specialization-scope.md`.

| Task ID | Role | User-observable leaf signal |
|---|---|---|
| **3485** | Grammar alternative on `sub_declaration` | `cargo test -p reify-syntax -- sub_decl_specialization_body_parses_from_source` passes; permitted/forbidden cases both parse, with rejection happening at the validator. |
| **3486** | Lowering to populate `SubDecl.body: Some(...)` | Existing `compile_pipeline_invokes_specialization_scope_validator` test rewritten to start from `.ri` source and passes. |
| **3488** | End-to-end fixture + LSP surface (leaf) | Forbidden-decl diagnostic squiggles appear in the GUI/LSP on the correct keyword+name span; permitted-only body emits zero diagnostics. |

**Closes:** findings/specialization-scope.md M-002 producer side; PRD AC 1-7
end-to-end. **Downstream unlocks** that need separate follow-up (NOT filed
here):
- shadow_lint recursion into `s.body` (findings/shadowing-warning.md M-016) —
  one-line walker extension once body is reachable.
- match-arm `sub` body / `where`-clause support (findings/match-block-decls.md M-006).

---

## (C) Skipped — mis-classified or shipped

### C1 — `docs/prds/kleene-logic.md` (`implies` operator)

**Reason:** PRD already correctly retired the operator-level path. The PRD
itself documents the YAGNI deferral and notes the de-Morgan rewrite
(`a implies b ≡ !a || b`) is the v0.1 path; it explicitly states
"When `BinOp::Implies` evaluation is wired, a `kleene_implies` function
and direct truth-table coverage should be reintroduced". The remaining
`implies` advertisement lives in the **language spec** (§15 grammar
summary, §16 precedence table) — spec drift is outside the grammar-fiction
triage's PRD-by-PRD brief.

**Evidence:** `docs/prds/kleene-logic.md` §2 ("Implementation Note —
`kleene_implies`"); findings/kleene-logic.md M-002 acknowledges the PRD's
own correct hedging.

**Follow-up flag for Phase 3 supervisor:** the spec-level claim is a
separate language-spec maintenance item; not filed as a task here because
the policy gate is per-PRD.

### C2 — `docs/prds/field-source-kinds.md` (`RegularGrid1` struct ctor)

**Reason:** This is explicitly a GR-001 case per audit M-016 — `RegularGrid1`
was retreated to string-tag dispatch (`grid = "RegularGrid1"` + separate
`bounds`/`spacing` keys) under escalation esc-2341-149 (2026-04-29) because
Reify lacked anonymous-struct-literal syntax + stdlib `RegularGrid*`
constructors. The PRD itself is the **inverse** shape (audit M-014 to M-024
all WIRED; PRD prose stale). The GR-001 follow-up PRD (umbrella for
struct-instance runtime representation) is the right home if/when Reify
gets typed `RegularGrid*` ctors.

**Evidence:** findings/field-source-kinds.md M-016 explicit cross-link to
GR-001; in-code comment at `crates/reify-compiler/src/functions.rs:319-327`.

**Follow-up flag for Phase 3 supervisor:** the field-source-kinds PRD is
described in findings as "a stale spec for a feature that has shipped" —
a separate prose-refresh sweep would update it to reality (Composed,
Imported v0.2, Sampled-via-string-tag), but that is not grammar-fiction
remediation.

---

## Cross-cutting observations

### O1. The auto-type-param grammar surface is shared across two PRDs (B1 chain de-dupes)

The v0.1 PRD (`auto-type-param-resolution.md`) is the **origin** of `auto:` in
type-arg position; the v0.2 PRD (`auto-resolution-backtracking.md`) extends the
algorithm but inherits the same grammar gap. Filing two parallel grammar
chains would have been wasted work; B1 covers both PRDs' user-observable
leaves. This is the largest "bundle into one chain" win in the triage.

### O2. Several PRDs assume "small grammar extensions" that are actually large

Three PRDs (varying-thickness-shells, imported-field-source-hdf5-csv,
match-block-decls with body+where) each describe a grammar extension as
"small" or "filed alongside" when in fact each is a non-trivial language
surface change requiring co-design across grammar, parser, lowering, and
sometimes runtime evaluation. The grammar-gate policy
(`feedback_prd_grammar_gate.md`) specifically catches this pattern; the
PRD-prose updates explicitly demote the "small" framing to "separate
grammar PRD required".

### O3. Two of the 13 cases were inverse — feature shipped, PRD just didn't authorize

- `forall`-statement `chain` body: grammar + AST + compiler all ship; PRD
  scope hadn't been widened to include it.
- The `field-source-kinds.md` PRD overall: every mechanism shipped; PRD prose
  describes an earlier moment in the feature's lifecycle.

This inverts the dominant grammar-fiction pattern. Worth Phase 3 awareness:
the grammar-fiction symptom is **bidirectional**, not just PRD-ahead-of-code.
The remedy here is the same — PRD-prose updates — but the diagnosis differs.

### O4. The annotation-args expansion is a load-bearing single grammar PRD waiting to be filed

`@shell(thickness = linear_taper(...))` (varying-thickness-shells PRD) and
`@allow(shadowing)` (shadowing-warning PRD) both implicitly request named /
keyword annotation arguments. The latter wants just a simple flag-arg form;
the former wants full expression evaluation with runtime semantics. These
are NOT the same grammar surface; the simple form is much smaller. Worth
filing a "named-arg annotation extension (flag-form)" PRD for v0.2-shaped
shadow-warning suppression, separate from any runtime-evaluable
annotation-args PRD for v0.5 varying-thickness shells.

### O5. None of the 13 cases were already-shipped grammar

The Phase 2 audit's grammar-fiction observations were all confirmed against
2026-05-12 `grammar.js` + ts_parser. No "audit was stale, this shipped
already" cases surfaced, which suggests the grammar-fiction category is
durable signal — unlike the runtime FICTION cluster C-07 where several
tasks were optimistically "done".

### O6. The grammar-fiction-gate policy is specifically validated by chain B1

The `auto:` in type-arg-list case is the textbook validation of
`feedback_prd_grammar_gate.md`: library Phase A/B/C orchestrator is fully
WIRED (35+ tests, the entire dispatch table works, determinism pinned), but
no `.ri` source can reach it because the parser doesn't accept the surface.
Every test hand-constructs `AutoTypeParam` instances. The gate policy would
have caught this at PRD-decompose time; the (B1) chain repairs it.
