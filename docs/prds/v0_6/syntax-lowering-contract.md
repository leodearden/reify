# PRD: Syntax lowering contract — one fault policy, one member-list lowerer, a grammar the lowering is checked against

**Status:** active — authored 2026-10-17/18 by the gap-fill stream session for **syntax-lowering-contract**
(`plans/gap-fill-deliberation-reify-20261016.md` §K2-D1, with §K2-D5, the uncontested block's K2-D6 and K2-D7,
and the lowering half of §K2-D2). Decomposed in the same session (2026-10-18): α #9343, β #9344, γ #9346, δ #9347, ε #9348, ζ #9349, η #9350, ω₁ #9351, ω₂ #9352, Ω #9353; follow-on milestone #9354. Shape: **B+H** (contract §6,
boundary-test sketch §7). Milestone: v0_6.

**Code anchors** verified against main `baff3ed38b` (2026-10-17). `git diff --stat 04be9302ea..a3c5ded78a` over
`crates/reify-syntax`, `tree-sitter-reify`, `crates/reify-ast/src` and `crates/reify-lsp/src` is empty, so the
survey's anchors still hold (the survey ran at `0e60fa138d`). Main moves fast: cite by symbol, and re-locate
lines at implementation time.

**Finding keys owned (7):** fk-3553b78476b5 (high), fk-653f242d7f93, fk-7173c189d7ea, fk-cff74023db39,
fk-258131627845, fk-50ed9b0483bf, fk-cdf6477a3b3d. All seven are hotspot-survey-reify-20261015 keys. Statements and proposals:
`docs/notes/bug-hotspot-survey-2026-10-15-full-findings.json`. §10 maps each key to its leaf.

---

## §1 Goal

reify refuses any module whose `ParsedModule.errors` is non-empty, so a parse fault that produces no
`ParseError` is a silent wrong program: the CLI says "All constraints satisfied" about source the parser did
not understand. Today the property that prevents this ("a CST fault yields at least one ParseError", the safety
half of INV-SF-7 `parse-is-value-faithful`) is decided separately in every dispatch arm of
`crates/reify-syntax/src/ts_parser.rs`, using three different mechanisms. Five arms or bodies still fail
open.

After this PRD:

- **One enforcement point.** `root.has_error() ⇒ !errors.is_empty()` is enforced once, at the parse entry
  (`parse_with_prelude_enums`). A redundant `debug_assert!` checks it at the same point, and a must-error test over
  the tree-sitter corpus and the repo's own probes pins it.
- **One refusal policy.** One `lower_guarded` combinator replaces `check_and_lower!`, `lower_function_checked` and
  `lower_enum`'s inline guard.
- **One member-list lowerer.** `lower_member_list` replaces the per-body loops, each of which decides
  annotation, pragma and ERROR handling for itself today.
- **The grammar's kind vocabulary is declared once and checked.** grammar.js gains `supertypes`, and a
  node-types.json contract test proves that every container's child kinds are handled or excluded for a stated
  reason. A new grammar kind then fails one test instead of being dropped with no diagnostic.
- **The unit operator is read from the CST.** It is no longer recovered by slicing source text between
  operands.
- **ts_parser.rs is split along its real seam.** It is 8,947 lines today. The split makes frozen pass-1
  `FileScope`, a `DiagSink` and an `ExprLowering` child module that imports only downward. The split lands
  last, on settled code (K2-D5).

User-observable: every CST-faulty probe in §2.2 changes from `reify check` rc 0 with "All constraints
satisfied." to rc 1 with a parse error located at the fault. Pragmas inside `where { }` blocks and
specialization bodies, and annotations inside specialization bodies, change from silently dropped to rejected
with a diagnostic (R7′).

## §2 Background: what the code does at `baff3ed38b`

### §2.1 Three refusal mechanisms (fk-3553b78476b5)

- `check_and_lower!` → `Lowering::refuse_if_faulty` **pre-refuses**: it never runs the inner lowerer, and it
  emits `invalid <label>: <snippet>` located at the first ERROR/MISSING descendant
  (`push_fault_error_with_excerpt`). It covers these `lower_source_file` arms: import, unit, type alias, joint
  and default. It also covers every `lower_member` arm except `associated_type` and the fn arms.
- `Lowering::lower_function_checked` **runs the inner lowerer, discards its result, then backstops**. It emits
  `syntax error in function definition` only when the inner pass was silent. Its doc explains why: a
  pre-emptive guard would replace the precise inner message with a vague one.
- `Lowering::lower_enum` and `lower_type_args_from_node` **report and keep a partial AST**.
- The module arm calls `refuse_if_faulty` by hand.
- These `lower_source_file` arms call their lowerer unguarded: structure, occurrence, trait, field, purpose,
  constraint_definition, annotation and pragma.
- `lower_member`'s `_ => None` and `lower_expr`'s `_ => None // Unknown node kind — skip` are fail-open
  defaults.

`check_and_lower!("guarded block")` in `lower_member` shadows `lower_guarded_block`'s own ERROR arm. So the
let-anchored diagnosis `diagnose_error_node` is unreachable inside `where` blocks.

**Who reads the AST of an errored parse.** Every production publisher gates on `errors` before it compiles:
`reify-cli` `parse_and_compile`, `gui/src-tauri/src/engine.rs::compile_single_file_with_stdlib`, and
`reify-lsp/src/diagnostics.rs::compute_diagnostics_with_state`. The doc of
`crates/reify-compiler/src/compile_builder/pre_pass.rs::forward_parse_errors` names those gates as
load-bearing. reify-lsp's editor providers, however, read `declarations` from an errored parse with no gate.
Outline (`symbols::compute_document_symbols_from_parsed`), hover and completion (`AnalysisContext::from_parsed`),
goto, rename and references all go through `DocumentState::parsed_module`. No test pins what those providers
return for a broken document. So the refusal *scope* is visible to the user in the editor while they type, and
nowhere else. Ruling R1 (§4) is based on this.

### §2.2 Silent accepts, measured (debug `reify` built 2026-10-16 14:45; no commit to the parser paths since)

Each fixture below is committed under `tests/prd-gate/fixtures/` as evidence. All parse with a CST
fault in the Rust runtime unless noted.

| fixture | source | `reify check` today |
|---|---|---|
| `slc_purpose_param_missing.ri` | `purpose P(x: ) { }` | rc 0, "All constraints satisfied." (#8080) |
| `slc_purpose_body_error.ri` | `purpose P(x: Real) { param y: }` | rc 0 (#8080) |
| `slc_trait_body_port.ri` | `trait T { param a : Length` / ` port p : out T }` | rc 0 (#7809; its branch adds the ERROR arm) |
| `slc_struct_header_fault.ri` | `structure S<T: > { param a : Real = 1 }` | rc 0 (#8275) |
| `slc_annotation_arg_fault.ri` | `@foo(1 +)` / `structure S { … }` | rc 0, only "unknown annotation @foo" (#8081) |
| `slc_field_analytical_fault.ri` | `field def F : Length -> Real { source = analytical { 1 + } }` | rc 0 (fk-653f242d7f93; no owner before this PRD) |
| `slc_field_imported_fault.ri` | `… imported { path = "a.vdb" format = } }` | rc 1, but only from two compiler errors ("missing required key: 'format'" and "… 'grid'"); the first blames a key the author wrote |
| `slc_where_pragma_dropped.ri` | `where x > 1mm { #frobnicate(1) }` in a structure (CST clean) | rc 0; the same pragma in a structure body warns "unknown pragma" |
| `slc_spec_body_pragma_dropped.ri` | `#frobnicate(1)` and `@deprecated("x")` in `sub motor : ElectricMotor { … }` (CST clean) | rc 0, both dropped |
| `slc_root_error.ri` | a token salad whose ROOT is `ERROR` in the Rust runtime | rc 1 with eight errors, among them `unexpected 'identifier' in source file`, `unexpected 'type_arg_list' …`, `unexpected 'number_literal' …` (#7810's debris cascade) |

S9's `tests/prd-gate/fixtures/decl_passes_purpose_body_sub_swallowed.ri` (#8732's B4) is the same class.

**Tracked corpus, measured:** of the 867 tracked `.ri` files (before this PRD's fixtures), 16 have a CST fault
in the Rust runtime, and 13 of those already report a parse error. One clean-CST file reports a parse error as
well: `docs/prds/v0_6/fixtures/indexed_sub_instantiation_surface.ri`, the live #5482 interim rejection in
`lower_sub`, found by the decompose-verify workflow's sweep. The three silent CST-faulty ones are
`examples/m9_trait_conformance.ri` and `examples/integration_corner_cases.ri` (the trait-body `port` lines that
esc-7809-5 removes on #7809's branch) and S9's B4 fixture. Both examples are read by eval harness tests
(`harness_milestone_integration/m9_trait_conformance.rs`, `harness_corpus_gates/eval_invariant_corpus_sweep.rs`),
so the backstop cannot land before #7809's corrections do (§9 α).

### §2.3 Member lists (fk-7173c189d7ea)

grammar.js's `commonMembers` is one list of 18 kinds that feeds `_member` (plus `associated_type` and
`function_definition`) and `_guard_member`. `specialization_body` is `repeat(choice($.param_assignment,
$._member))`. The lowering has no counterpart to it. These loops each decide annotation, pragma and ERROR
handling for themselves:

- `lower_members` (structure and occurrence);
- `lower_guarded_block`;
- `lower_trait_members`;
- `lower_purpose_members`;
- `lower_specialization_body_members`;
- `lower_port_body`;
- `lower_constraint_def`'s body loop.

`lower_derived_sub`'s body loop and `lower_sketch_block` have the same shape: `lower_sketch_block` ends in
`_ => {}`. The policies differ:

- **Annotations** attach to Param/Let in `lower_members` and `lower_guarded_block`, to Fn in
  `lower_trait_members`, and nowhere in `lower_specialization_body_members`.
- **ERROR children** get three treatments: `diagnose_error_node`, `push_fault_error_with_excerpt`, or a
  silent drain (trait and purpose bodies).

`lower_members` feeds `lower_member` every child of the declaration, including `{`, the name, the
`type_parameters` and the `trait_bound_list`. That is why `lower_member`'s `_ => None` cannot be a diagnostic
today.

### §2.4 Kind vocabulary (fk-cff74023db39)

`tree-sitter-reify/src/node-types.json` gives the exact child kinds each container can produce. It is
generated by `tree-sitter-reify/build.rs` and gitignored, and no crate reads it. grammar.js declares no
`supertypes`. The lowering re-lists kinds as string literals in `lower_source_file`, `lower_member`,
`lower_expr` and every body loop.

`member_continuation.rs` keeps a third hand list, `MEMBER_LIST_CONTAINERS`. Its drift guard
(`member_list_containers_cover_every_member_repeat_in_the_grammar` in
`tests/harness_syntax/member_continuation_ambiguity_tests.rs`) re-lexes grammar.js with ~250 lines of ad-hoc
JS scanning (#7676). It is also coupled to grammar.js formatting.

Four inline tests feed private lowerers deliberately wrong node kinds, solely to reach catch-all arms that
`parse()` cannot reach: `lower_connect_body_…`, `lower_port_body_…`, `lower_constraint_def_…` and
`lower_source_file_catch_all_…`.

**Probed in a scratch copy (tree-sitter CLI 0.26.8; the production runtime is the Rust crate 0.24.7):**

- **`supertypes`.** Adding `supertypes: $ => [$._expression, $._member, $._guard_member, $._declaration]`
  generates cleanly, and the unchanged corpus passes 263/263. `root.to_sexp()` with field names is
  byte-identical over all 867 tracked `.ri` files in the Rust runtime.
- **node-types.json after that edit.** It gains four `subtypes` entries: `_declaration` 15, `_expression` 28,
  `_guard_member` 18 and `_member` 20. Container entries now reference the supertype instead of the flat kind
  list.
- **The CLI and the Rust runtime recover from errors differently** on the same `parser.c`. Corpus
  S-expressions are CLI output, so they do not pin what the lowering sees; §6.1 states how the must-error test
  handles this.
- **Corpus fault entries.** The corpus holds 263 entries, and only 8 expect ERROR/MISSING:
  `auto_operand_rejection.txt` ×6, `imaginary_literal.txt` ×1 and `interpolated_string.txt` ×1.

### §2.5 Unit operator (fk-50ed9b0483bf)

`scanner.c` emits typed `UNIT_MUL_OP` and `UNIT_DIV_OP` tokens. `grammar.js::unit_expr` binds them as
`field('op', choice($._unit_mul_op, $._unit_div_op))`, which is hidden, so `child_by_field_name("op")` is
`None`. `lower_unit_expr` therefore slices `self.source[left.end_byte()..right.start_byte()]`, classifies the
text (`classify_unit_op`), and falls back to stripping comments (`collect_unit_op_comment_spans`,
`strip_unit_op_comments`; #6567, fixed by #6952 with the strip pass). With
`alias($._unit_mul_op, '*')` / `alias($._unit_div_op, '/')` (probed):

- generation is clean;
- the corpus passes 263/263;
- the Rust S-expressions are identical over the 867 files;
- `child_by_field_name("op")` returns an anonymous node of kind `*` or `/`. `·` is kind `*` with text `·`.
  In `5(m)/*c*/*(s)` the op node excludes the comment;
- `parser.c` changes by 8 lines.

`parser.c` is gitignored, so every grammar edit costs a regeneration only (freshness gate
`scripts/tree-sitter-freshness.sh`, #5629).

### §2.6 ts_parser.rs size (fk-258131627845, fk-cdf6477a3b3d)

The file has 6,126 production lines plus a 2,820-line inline `mod tests` holding 91 tests. `Lowering::new(`
occurs 14 times in the file, almost all of them in tests that construct the lowering directly. #8182 moves the
module out to `ts_parser/tests.rs`.

The expression partition is lines ~4,260–5,877 (the survey's (D)). It is all `&self` and reads only the three
pass-1 tables (`known_enums`, `namespace_bindings`, `entity_bindings`) plus the error sink. A child file holding
`impl super::Lowering` would reach back into the parent's private fields. That is the heuristic-13 stitching the
quality definition rejects, so the split is blocked by state lifetime (heuristics 5 and 8), not by the code.

There are also two kinds of prose to fix:

- 12–13 hard `ts_parser.rs:NNN` / `~line NNN` / `grammar.js:NN` cites. Sampled cites are stale.
- Merge-surface sentences about task/5392, which is done: in `parse_with_prelude_enums` and in
  `member_continuation.rs`'s module doc.

## §3 Pre-conditions and already-filed owners

- **#7809** (pending, branch `task/7809` seven commits ahead): the trait-body interim fix. It carries
  esc-7809-5's corrections to both examples, the ≤3-line grammar.js breadcrumb above `trait_member`, and the
  `lower_trait_members` ERROR arm. α depends on it, because the backstop would red the two examples it
  corrects. δ subsumes its arm.
- **#8182** (pending, low): moves the inline `mod tests` to `ts_parser/tests.rs`, as K2-D5 B rules. ε and ζ
  depend on it, because the tests they delete move with it. ω₁ and ω₂ depend on it.
- **S9 μ #8742** (pending, low): `ParseErrorKind` on `ParseError`. Ruling R8 asks it for an `UnattributedFault`
  variant, and η depends on it.
- **S9 σ #9322** (pending, high, gate #9171): `SpannedIdent` fields, **including** their population at the
  ts_parser.rs lowering sites (ruling R4). ω₂ depends on it, because σ edits `lower_function_call` and that
  function moves into `expr.rs`.
- **S8 π #8904** (pending, low): a one-hunk edit to `lower_binary_expr`. ω₂ depends on it for the same reason.
  This is a liveness note for the lead, not a defect: the close, and with it #9150's edge, waits transitively on
  #8904 → #8620 → #8613 (an S7 chain, all pending at 2026-10-18) and on #9322.
- **S9 γ #8732** (pending, medium): owns the compiler-side `member_support` table. Its parser-side hunk is
  ceded, or it lands first as an interim fix. No edge (ruling R5). File locks on `ts_parser.rs` serialise the
  two.

## §4 Rulings

### From the record (binding; verbatim headlines, full text in `plans/gap-fill-deliberation-reify-20261016.md`)

- **K2-D1** (human, 2026-10-16): A, with a consolidation instruction. Adopt a new agent stream
  `syntax-lowering-contract` owning reify-syntax's lowering dispatch, body loops and fault policy:
  - a module-level `fault_backstop(root, &errors)` at `parse_with_prelude_enums`, plus a debug assert and a
    corpus-driven must-error test over every tree-sitter corpus entry containing ERROR/MISSING (high, gate
    #9171);
  - one `lower_guarded` combinator with one refusal policy;
  - `lower_member_list(container, &MemberListSpec)` replacing the seven body loops;
  - grammar.js `supertypes` plus a node-types.json contract test (absorbs #7676's lexer replacement);
  - the `field_definition` arm;
  - an examples/corpus sweep for newly-red silent drops;
  - a close leaf (added to S23 θ₃ #9150's dependencies per S23 OQ-7 (a)).

  **Leo's addition:** the residual site fixes #7809, #8080, #8081, #8275 and #7810 are combined into the
  stream's leaves as far as is appropriate. Content is ported before any is cancelled with `x_subsumed_by`.
  #7809's Leo-ruled example corrections and grammar.js breadcrumb (esc-7809-5) are carried as ruled.
- **K2-D5** (human, 2026-10-16): B now, A as the syntax-lowering stream's closing leaf. #8182 moves the tests
  out. The stream's last leaf:
  - freezes the pass-1 tables into `FileScope`;
  - adds `DiagSink` and an `ExprLowering<'a> { source, scope, diag }` child module that imports only
    downward;
  - moves the remaining internals-reaching tests to harness_syntax_lowering;
  - adds a grep ratchet forbidding `impl super::Lowering` / `use super::*` in `ts_parser/*.rs`.
- **K2-D6** (uncontested block): a leaf of the syntax-lowering stream. `unit_expr`'s op is exposed via an
  anonymous grammar alias, and `classify_unit_op`, `collect_unit_op_comment_spans`, `strip_unit_op_comments`
  and UnitOp's Missing/Unrecognized arms are deleted.
- **K2-D7** (uncontested block): the reify-syntax prose belongs to this stream: stale cites, the task/5392
  merge-surface sentences, and INV-SF-7 rule rehoming to design-invariants.md §INV-SF-7. S23 θ₃ #9150 gains
  the stream's close leaf.
- **K2-D2** (human, 2026-10-16): S9 σ owns the `SpannedIdent` fields, which are populated at the lowering
  site by this stream. Ruling R4 below re-assigns the population to σ.
- **G1** (human, 2026-10-16): #8612's transitive dependency closure is protected. No leaf here becomes a
  prerequisite of a closure task. Checked live on 2026-10-17: none of #9171, #9150, #8182, #8732, #8742, #8904,
  #8822, #9321, #9322, #7809, #8080, #8081, #8275, #7810 or #7676 is in the closure (245 tasks).

### This session (Leo, 2026-10-17/18; answers verbatim)

- **R1 — refusal policy** (K2-D1's open question). "(a) + leaves only (Recommended)". Run the inner lowering
  for its diagnostics, then refuse the node. Emit `invalid <label>: <excerpt>` only if the inner pass was
  silent. Apply it to **leaf** nodes, never to containers; a container's faulty members are refused instead.
  `check_and_lower!`, `lower_function_checked` and `lower_enum`'s inline guard are deleted.
- **R2 — consolidation map.** "Approve map (Recommended)". The map is §11.
- **R3 — #7676.** "Cancel, subsumed". ε ports #7676's acceptance (four assertions, two non-vacuity guards,
  the six helpers deleted) and #7676 is cancelled with `x_subsumed_by` ε.
- **R4 — SpannedIdent lowering.** "No leaf here". σ #9322 does the fields, their population at the lowering
  sites, and the tests. This stream files no SpannedIdent leaf. Its only obligation is that ω₂ lands after σ.
- **R5 — #8732's parser hunk.** "No edge (Recommended)".
- **R6 — the close.** "Two consecutive leaves (Recommended)": ω₁, then ω₂, both after every other leaf.
- **R7 — pragmas and annotations inside `where` blocks and specialization bodies.** "Reject loudly, file a
  milestone for pragmas in guards after the review-all work capstone". Their member-list specs reject both with
  a located diagnostic. A pending decision milestone that depends on #8612 (the review-all program's completion
  trigger) asks whether to support them (§12).
- **R7′ — correction to R7's premise** (Leo, 2026-10-18; asked after the decompose-verify workflow found that
  annotations inside `where` blocks are attached, not dropped, and are pinned by
  `solver_hint_tests::solver_hint_in_guarded_block_compiles` and
  `display_annotation_tests::display_wrong_ladder_on_guarded_param_errors`). "Reject only what is dropped
  (Recommended)":
  - pragmas in `where` blocks and in specialization bodies are rejected;
  - annotations in specialization bodies are rejected;
  - annotations in `where` blocks keep attaching to Param/Let as today.

  The milestone covers pragmas in guards and annotations in specialization bodies.
- **R8 — `ParseErrorKind::UnattributedFault`.** "Yes, ask μ". The variant is requested on #8742 at filing,
  by a dated details append on the task record, which is recorded in the FILED row. η depends on #8742, gives
  the backstop that kind, and adds the "backstop is never the sole diagnostic" ratchet.

## §5 Resolved design decisions

- **D1 — the backstop rule.** `fault_backstop` is pure CST plus read-only errors, and lives in
  `ts_parser/fault_diagnosis.rs`, whose module rule is "reads no lowering state".
  `first_error_or_missing_descendant` moves there with it.
  - **Rule.** For each direct child `c` of the root with a CST fault, emit one `ParseError` when no emitted
    error's span intersects `c`'s byte range. The intersection is inclusive at both ends, because a MISSING
    node is zero-width. The error is located at `first_error_or_missing_descendant(c)` and reads
    `syntax error in <c's kind with underscores as spaces>`. An `ERROR` child counts like any other.
  - **Granularity is per faulty top-level child, not per fault.** That is the whole safety property: one
    error refuses the module. Precision inside a child is message quality (η).
  - **Order in `parse_with_prelude_enums`:** lower, then `check_member_continuations`, then the backstop, then
    `debug_assert!(!root.has_error() || !errors.is_empty())`. The backstop constructs the invariant and the
    assert checks it where `ParsedModule` is built (heuristic 10, redundantly).
- **D2 — root `ERROR` (#7810) is lowering policy, not backstop policy.** When `root.is_error()`,
  `lower_source_file` dispatches only the declaration kinds it handles; any other named child is recovery
  debris and gets no `unexpected '<kind>' in source file`. Then `diagnose_error_node(root, "source file")` runs
  once. On a well-formed root, the `warn_unexpected_child` default stays, because there an unhandled named
  child means grammar drift (ε turns it into a contract-test failure).
- **D3 — leaves and containers (R1).** A **container** is a node whose non-header content is member lists:
  `structure_definition`, `occurrence_definition`, `trait_declaration`, `purpose_declaration`,
  `constraint_definition` and `guarded_block`. Every other dispatched kind is a **leaf**:
  - import, unit, type alias, joint, default, module, function, enum, field, annotation and pragma at top
    level;
  - every member arm except `guarded_block`.

  `sub` is a leaf. Its header, arguments and specialization or derived body form one value, so a fault
  anywhere in it, body included, refuses the whole `sub`. That is today's behaviour too (`check_and_lower!` on
  `sub_declaration`). Under policy (a) the body's member list still runs first, so a faulty body member is
  diagnosed and located; the `sub` then leaves `members`, while its container (and the LSP outline entry)
  stays.

  A container is never refused whole. Its faulty members are refused, and a header fault is attributed by the
  backstop (safety) and by η (message). The distinction is one column of the dispatch table (ε), not a second
  policy.
- **D4 — one combinator.** `lower_guarded(node, label, f)` runs `f`. If `node` has a CST fault it returns
  `None` and, when `f` pushed nothing, pushes one `invalid <label>: <snippet>` at the node's first fault.
  Nested guards compose: an inner report shows as error-count growth, so the outer guard stays silent.
  - `syntax error in function definition` and `syntax error in enum declaration` become the `invalid <label>:`
    shape. No test pins either string.
  - The `invalid <label>: <excerpt>` shape survives for faults the inner lowering does not itself report,
    e.g. the MISSING-shaped `connect a -> b { shaft -> }` cases in `check_and_lower_snippet_bound_tests.rs`.
  - Where an inner arm already reports a fault, the inner message now wins, because the outer guard sees error
    growth and stays silent. This changes three pinned cases in `check_and_lower_fault_location_tests.rs`, and
    γ re-pins them deliberately (R1):
    - the port body ERROR case (`port a : in T { >= ) <= ( }`) becomes `syntax error in port body`, located at
      the ERROR (`lower_port_body`'s arm);
    - the connect body ERROR case becomes `syntax error in connect body`, located at the ERROR
      (`lower_connect_body`'s arm);
    - the guarded-block case (`where x > 0 { let y = }`) becomes the refused member's own `invalid let: …`,
      located at the missing value, because `guarded_block` is a container. `invalid guarded block:` is no
      longer emitted.
  - `lower_type_args_from_node`'s diagnostic stays as a precise inner message.
  - `lower_guarded_block` leaves the `lower_member` match and is lowered as a container. That makes its
    let-anchored ERROR diagnosis reachable inside `where` blocks for the first time.
- **D5 — the member-list value.** It is a per-container `MemberListSpec` (§6.3). There is one per container
  kind, with names such as `STRUCTURE_BODY`, `TRAIT_BODY`, `PURPOSE_BODY`, `GUARDED_BLOCK`,
  `SPECIALIZATION_BODY`, `PORT_BODY` and `CONSTRAINT_DEF_BODY`. `lower_member_list` takes one brace-delimited
  body, obtained from `member_bodies(container)`. That function generalises `member_continuation::members_of`:
  it returns one body per top-level `{…}` pair, so `guarded_block` yields main and `else`, and every other
  container yields one. `members_of` becomes its single-body case.

  This refines the ruled signature `lower_member_list(container, &MemberListSpec)`. The intent is kept: one
  function, one spec per container. ts_parser already depends on `member_continuation`, so no new module edge
  appears (INV-ARCH-1).

  **Where the code lives (heuristic 13).** `ts_parser/member_list.rs` holds only data: `MemberListSpec`,
  `MemberList`, `AnnotationTarget`, `PragmaPolicy` and the per-container spec constants. It imports nothing
  from its parent. `lower_member_list` is a `Lowering` method and stays in `ts_parser.rs` with the other (C)
  member lowerers. A `&mut self` lowerer in a child file would be `impl super::Lowering` stitching, the shape
  ω₂'s ratchet forbids.
- **D6 — what a spec decides, per child, in one place.**

  | child | handling |
  |---|---|
  | `annotation` | `lower_guarded`, then pending; or Reject: `annotation '@x' is not supported in <context>` |
  | `pragma` | `lower_guarded`, then collected; or Reject: `pragma '#x' is not supported in <context>` |
  | `ERROR` | drain pending annotations, then `diagnose_error_node(child, spec.context)` (one mechanism everywhere) |
  | kind in `caller_owned` | handed back untouched |
  | kind in `members` | drain pending, `lower_member`, attach per `spec.annotations` |
  | any other named non-extra kind | `warn_unexpected_child(child, spec.context)` |

  No annotation node is silently dropped. Where the grammar yields a dangling annotation node at the end of a
  body, it is reported. In a structure body a trailing `@foo` is already an ERROR node and is diagnosed by the
  ERROR arm. `lower_member`'s `_ => None` becomes a diagnostic, reachable only through grammar drift, which
  ε's test reds first.

  Per R7′, `guarded_block` uses `PragmaPolicy::Reject` with `AnnotationTarget::ParamAndLet`, which keeps
  today's attachment. `specialization_body` uses `PragmaPolicy::Reject` and `AnnotationTarget::Reject`.
- **D7 — the vocabularies stay apart, bridged by `MemberDecl`.** The parser's spec sets state *grammar
  admission*: CST kind strings whose truth is node-types.json. S9 γ's `member_support(MemberKindTag,
  MemberContext)` states *semantic support*. They differ on purpose: the grammar admits `port_declaration` in a
  `where` block, and γ rejects it. Merging them would make one of the two lie (heuristic 1).

  The shared vocabulary already exists: `MemberDecl`, which `lower_member` produces and γ's `MemberDecl::tag()`
  names. γ's B2 matrix can feed its "every grammar-admitted (container, kind) cell has a `member_support` row"
  check from `reify_syntax::grammar_contract` (D8). That is a seam note for S9 (§8), not a leaf here.
- **D8 — a public, read-only contract surface instead of internals.** `ts_parser` is private, so the contract
  test cannot name its statics. Following `member_continuation::MEMBER_LIST_CONTAINERS` (public "so the
  contract is readable and checkable from outside the crate"), `lib.rs` exports `pub mod grammar_contract`.
  Its functions and constants are built from the **same statics the dispatch iterates**, so test and dispatch
  cannot drift (heuristic 11), and the test drives a public seam (Tests stance). The dispatch rows name
  `Lowering` methods, so they live beside the dispatch in ts_parser.rs. A child module holding
  `fn(&mut super::Lowering, …)` pointers is exactly the reach-back ω₂ ratchets against.
- **D9 — #7676 is absorbed (R3).** node-types.json states child sets, while grammar.json states which rules
  own a separator-free `repeat` (the member-continuation fact). ε hosts both reads in one serde_json-equipped
  test module. The grammar.json walk replaces the ~250-line grammar.js lexer, keeps the drift guard's four
  assertions and two non-vacuity guards, and derives `MEMBER_LIST_CONTAINERS`' covered set.
- **D10 — the unit op from the CST.** `lower_unit_expr` matches `child_by_field_name("op")?.kind()` on `"*"`
  and `"/"`. Any other kind is unreachable after the alias; it is reported, not panicked on. The `·` spelling
  stays a scanner concern.
- **D11 — the close is two leaves (R6).**
  - **ω₁** introduces `FileScope` (`ts_parser/scope.rs`, immutable after `FileScope::collect`) and `DiagSink`
    (`ts_parser/diag.rs`: source, the `RefCell` error vec, `push_error`, `push_fault_error(_with_excerpt)`,
    `snippet`, `span`, `node_text`). `Lowering`'s pass-1 fields are replaced by one `FileScope`, and the guard
    combinator runs over the sink. ω₁ is behaviour-preserving and moves no files beyond the two new ones.
  - **ω₂** does the moves:
    - (D) to `ts_parser/expr.rs` as `ExprLowering<'a> { source, scope: &FileScope, diag: &DiagSink }`, which
      imports only `super::{scope, diag}`. Four `&self` helpers defined outside the (D) range but called from
      it move down with it: `lower_binding_value` (also called by (C), which then calls
      `self.expr.lower_binding_value(…)`), `lower_auto_params`, `content_hash` and `dot_join`. `node_text`,
      `span` and `push_error` are `DiagSink`'s;
    - literal helpers to `ts_parser/literals.rs`;
    - `ts_parser/tests.rs` dissolved into `tests/harness_syntax_lowering`. #8182 lands that file with a
      module-level `use super::*`, which the ratchet forbids. Tests that construct `Lowering::new` are
      rewritten onto `parse()` or deleted where ε's contract test covers them;
    - the two ratchet rows (§6.6);
    - the K2-D7 prose.
- **D12 — the corpus guard comes first (α).** Every later leaf turns some silence into an error, so the corpus
  check is a standing guard landed before them, not a closing sweep. Every tracked `.ri` outside a declared,
  reasoned must-error list must have a clean CST in the Rust runtime and an empty `errors`. Each later leaf
  that reds a tracked file fixes it in the same diff, and the guard is what catches it.

  Each entry carries a reason kind: `CstFault` (a faulty CST) or `ErrorsOnly` (a clean CST whose lowering
  reports). The second kind is how δ's two clean-CST pragma fixtures are declared. The list names fixture
  paths in a tracked `.rs`, so `tests/infra/test_verify_scope.sh` Scenario PG-DRIFT requires every named
  `tests/prd-gate/fixtures/*.ri` to be registered in `scripts/verify.sh`'s `_RUST_COUPLED_RI_FIXTURES`. The leaf
  that adds an entry registers it in the same diff. `scripts/verify.sh` is a verify-pipeline file
  (`scripts/verify-pipeline-guard.sh requires-full-gate`), which the merge queue's full gate pays anyway.

## §6 Contract (B+H)

### §6.1 Backstop — `ts_parser/fault_diagnosis.rs`

```rust
/// INV-SF-7 safety half, enforced once. Precondition: lowering has run; `errors` holds every
/// diagnostic it emitted. Postcondition: for every direct child `c` of `root` with a CST fault,
/// some element of `errors ∪ result` has a span intersecting `c`'s byte range (inclusive;
/// zero-width MISSING included). At most one entry per faulty child, located at
/// `first_error_or_missing_descendant(c)`, message `syntax error in <humanised kind>`.
/// Reads no lowering state; emits no source excerpt.
pub(super) fn fault_backstop(root: Node<'_>, errors: &[ParseError]) -> Vec<ParseError>;
```

`parse_with_prelude_enums` ends with
`debug_assert!(!root.has_error() || !errors.is_empty(), …)`, and the assert message names the invariant.

**Must-error test** (`tests/harness_syntax/fault_backstop_tests.rs`, driving `reify_syntax::parse` only):

1. **Corpus.** Walk `tree-sitter-reify/test/corpus/*.txt`, which is tracked. For every entry whose expected
   S-expression contains `(ERROR` or `(MISSING`, assert `parse(input).errors` is non-empty and name the file
   and entry on failure. Because the corpus is CLI output (§2.4), the test first asserts that the Rust tree
   also `has_error()`. A divergence reds with its own message ("runtime disagrees with corpus"), never as a
   backstop failure.
2. **Probes.** A table of `(source, fault_byte_offset)` rows, each asserting at least one error whose span
   intersects the offset (located, not merely present). It holds:
   - the ten §2.2 fixtures;
   - #8080's two cases and its loud `let y =` control;
   - #8081's three: `@foo(1 +)`, `@foo(1,, 2)`, `#version(0.1,,)`;
   - #8275's seven header cases and three controls;
   - #7810's nine-error fixture, which must carry no `unexpected '…' in source file` on a well-formed line;
   - α's tracked must-error list.
3. **Non-vacuity.** At least 8 corpus fault entries are found, and the probe table has at least 20 rows.

### §6.2 One refusal policy

```rust
/// Runs `f` whether or not `node` is faulty. Clean node: returns `f`'s result. Faulty node:
/// returns None; if `f` pushed no diagnostic, pushes exactly one `invalid <label>: <snippet(node)>`
/// located at `node`'s first fault. Applied to leaf nodes only (D3).
fn lower_guarded<T>(&mut self, node: Node<'_>, label: &str,
                    f: impl FnOnce(&mut Self) -> Option<T>) -> Option<T>;
```

The function keeps the name `lower_guarded` through ω₁ (where it runs over the `DiagSink`), so γ's delivered
checks stay true.

### §6.3 Member lists — data in `ts_parser/member_list.rs`, the lowerer in `ts_parser.rs` (D5)

```rust
pub(super) struct MemberListSpec {
    pub context: &'static str,                 // "structure body", "trait body", "guarded block", …
    pub wrapper: Option<&'static str>,         // Some("trait_member") | Some("purpose_member") | None
    pub members: &'static [&'static str],      // CST kinds routed to lower_member
    pub caller_owned: &'static [&'static str], // handed back: default_declaration, structure_definition,
                                               // param_assignment, port_direction_setting, …
    pub annotations: AnnotationTarget,         // ParamAndLet | Fn | Reject
    pub pragmas: PragmaPolicy,                 // Collect | Reject
}
pub(super) struct MemberList<'t> { pub members: Vec<MemberDecl>, pub pragmas: Vec<Pragma>,
                                   pub caller_owned: Vec<Node<'t>> }
/// Invariant: every named non-extra child of `body` is exactly one of
/// {annotation, pragma, ERROR, member, caller_owned, unexpected → diagnostic}; none is skipped;
/// a pending annotation never crosses an ERROR or the end of the body silently.
fn lower_member_list(&mut self, body: MemberBody<'t>, spec: &MemberListSpec) -> MemberList<'t>;
pub(crate) fn member_bodies(container: Node<'t>) -> Vec<MemberBody<'t>>; // member_continuation
```

Adopting bodies:

- `lower_members` (structure and occurrence);
- `lower_guarded_block` (two bodies);
- `lower_trait_members` (it must keep #7809's `trait_tests.rs` green);
- `lower_purpose_members`;
- `lower_specialization_body_members`;
- `lower_port_body`;
- `lower_constraint_def`'s loop;
- `lower_derived_sub`'s loop;
- `lower_sketch_block`.

These keep their own loops, each excluded in ε's test with a reason:

- `connect_body`: comma-separated mappings, no member kinds. Its ERROR arm calls the same helper.
- `relate_block`, `sub_relate_block`, `joint_body`: `relation_member` expression lists.
- `keyed_member_block`: entries, each holding a specialization body that does go through the spec.
- `field_source_sampled` and `field_source_imported`: config entries (η).
- `meta_block`, `match_arm_decl_block`, `fn_body`: their own item kinds.

### §6.4 Grammar contract — `lib.rs`

```rust
pub mod grammar_contract {
    /// (container kind, handled child kinds), from the same statics the dispatch iterates.
    pub fn handled_child_kinds() -> &'static [(&'static str, &'static [&'static str])];
    /// (container kind, reason) — containers whose children the lowering deliberately does not route.
    pub const EXCLUDED_CONTAINERS: &[(&str, &str)];
    /// (container kind, child kind, reason) — child kinds a handled container carries that the member
    /// dispatch deliberately does not route (e.g. structure_definition's type_parameters / trait_bound_list,
    /// lowered by the header code).
    pub const EXCLUDED_CHILD_KINDS: &[(&str, &str, &str)];
    /// (kind, reason) — EXPR_LOWERERS keys that are not `_expression` subtypes (e.g. auto_seed).
    pub const EXCLUDED_EXPRESSION_KINDS: &[(&str, &str)];
    /// DECLARATION_LOWERERS keys and EXPR_LOWERERS keys.
    pub fn declaration_kinds() -> &'static [&'static str];
    pub fn expression_kinds() -> &'static [&'static str];
}
```

Contract test (`tests/harness_syntax/grammar_contract_tests.rs`) reads `tree-sitter-reify/src/node-types.json`
and `src/grammar.json`, both regenerated by `build.rs` before the test links, using serde_json as a
dev-dependency. It asserts:

- For each container that `handled_child_kinds()` names: the node-types `children` (expanded through supertype
  `subtypes`), plus only its member-bearing fields, equal the handled kinds plus that container's
  `EXCLUDED_CHILD_KINDS` rows. Expression-valued fields such as `guarded_block.condition` are checked by the
  expression assertion instead. Equality is checked in **both** directions: an unhandled kind reds, and a
  handled or excluded kind the grammar no longer produces reds. That second direction is #7676's "no dead
  entries" assertion.
- `subtypes(_declaration)` equals `declaration_kinds()` plus `annotation` and `pragma`, which the table handles
  as pending attachments. `module_declaration` is not a `_declaration` subtype: `source_file` carries it
  separately, and `lower_source_file` handles it before the table. It is therefore not an exclusion entry.
- `subtypes(_expression)` (flattened) equals `expression_kinds()` minus `EXCLUDED_EXPRESSION_KINDS`.
- #7676's repeat-site assertions hold over grammar.json.
- Non-vacuity: at least 10 containers are checked, `structure_definition` is present by name, and at least 15
  repeat sites are found.

`lower_source_file` and `lower_expr` become lookups in `DECLARATION_LOWERERS` / `EXPR_LOWERERS`.
`lower_expr`'s unknown-kind default reports `unexpected '<kind>' in expression` (an `ERROR` in expression
position included).

### §6.5 Unit operator

`unit_expr`: `field('op', choice(alias($._unit_mul_op, '*'), alias($._unit_div_op, '/')))`.
`lower_unit_expr` reads the op node's kind (D10) and reads no source text between operands.

### §6.6 Close — `ts_parser/{scope,diag,expr,literals}.rs`

```rust
pub(super) struct FileScope<'a> { known_enums: HashSet<&'a str>, namespace_bindings: HashSet<String>,
                                  entity_bindings: HashMap<String, ImportKind> }
impl<'a> FileScope<'a> { pub(super) fn collect(root: Node, source: &'a str, prelude_enum_names: &[&'a str]) -> Self }
pub(super) struct DiagSink<'a> { source: &'a str, errors: RefCell<Vec<ParseError>> }
pub(super) struct ExprLowering<'a> { source: &'a str, scope: &'a FileScope<'a>, diag: &'a DiagSink<'a> }
```

`collect_import_bindings` calls `lower_import`. The binding classification moves to `scope.rs` as a pure
function that both share; `lower_import`, the AST producer, stays in the parent.

**Ratchet rows** (`tests/infra/adoption-ratchet.manifest`, evaluated by `tests/infra/adoption_ratchet.py`).
ω₂ adds all three at 0:

- `ts-parser-child-impl-lowering | ^impl(<[^>]*>)? (super::)?Lowering[ <{] | crates/reify-syntax/src/ts_parser/*.rs | 0`
  catches both `impl<'a> super::Lowering<'a> {` and `use super::Lowering;` + `impl Lowering {`.
- `ts-parser-child-lowering-import | ^use super::(\{([^}]*[ ,])?)?Lowering[^A-Za-z_] | crates/reify-syntax/src/ts_parser/*.rs | 0`
  catches `use super::Lowering;` and `use super::{X, Lowering}`, but not `expr::ExprLowering`.
- `ts-parser-child-glob-import | ^use super::[*] | crates/reify-syntax/src/ts_parser/*.rs | 0`

All three pass `adoption_ratchet.py::_non_portable_construct` (checked 2026-10-18) and are anchored at column
0. A nested `#[cfg(test)] mod tests { use super::*; }` imports its own file's items, not the parent's, and is
not the heuristic-13 symptom. ω₂ also adds the census row `ts_parser child-module reach-back |
adoption-ratchet ts-parser-child-* | shipped` to `docs/legibility/design-invariants.md`'s umbrella table.

## §7 Boundary-test sketch

| scenario | preconditions | postconditions |
|---|---|---|
| Tracked corpus guard (α) | every tracked `.ri` | not in `MUST_ERROR_RI_FIXTURES` ⇒ Rust CST clean ∧ `errors` empty; in it ⇒ CST faulty (reason stated) |
| Corpus must-error (β) | the 8 corpus fault entries | Rust tree `has_error`; `parse(input).errors` non-empty; the debug assert does not fire |
| Probe table (β) | §6.1 rows | at least one error intersects each fault offset |
| Root ERROR (β, #7810) | `slc_root_error.ri`; #7810's fixture | no `unexpected '…' in source file`; at least one located error |
| Attributed child not doubled (β) | `structure S { let y = }` | exactly one error (`invalid let: …`); none from the backstop |
| Inner message preserved (γ) | the #5392 t9 fn body (`let x0 = 1` / `x0 * 2`, no `;`) | the let-anchored message survives; no generic `invalid function definition` is added |
| Silent inner backstopped (γ) | `import a.1.b as pp` | one `invalid import: …` at the fault |
| Annotation refused (γ, #8081) | `slc_annotation_arg_fault.ri` | rc 1; `invalid annotation: @foo(1 +)` located at the MISSING operand |
| Container kept (γ/δ, R1) | `structure S {` / ` param x: Real = 1` / ` let y =` / `}` | `S` stays in `declarations` (LSP outline keeps it); `S.members` holds `x` only; one located error |
| Sub refused whole, member located (γ/δ, D3) | `structure S { sub motor : M { let y = } }` with `M` defined | one `invalid let: …` located at the missing value; `S.members` lacks `motor`; `S` stays in `declarations` |
| Pinned messages re-pinned (γ, D4) | the port, connect and guarded-block cases of `check_and_lower_fault_location_tests.rs` | `syntax error in port body` / `syntax error in connect body` located at the ERROR; `invalid let: …` for the guarded block; the MISSING-only `invalid connect:` cases unchanged |
| Where-block pragma loud (δ, R7′) | `slc_where_pragma_dropped.ri` | rc 1; `pragma '#frobnicate' is not supported in guarded block` |
| Where-block annotation kept (δ, R7′) | `solver_hint_in_guarded_block_compiles`, `display_wrong_ladder_on_guarded_param_errors` | both unchanged and green: the annotation attaches to the guarded param |
| Specialization body loud (δ, R7′) | `slc_spec_body_pragma_dropped.ri` | one diagnostic each for the pragma and the annotation; `shaft_diameter = 8mm` still applied |
| Trait body (δ ⊇ #7809) | `slc_trait_body_port.ri` | #7809's `trait_tests.rs` assertions hold unchanged |
| Purpose body (δ ⊇ #8080, #8732 B4) | `slc_purpose_body_error.ri`, `decl_passes_purpose_body_sub_swallowed.ri` | rc 1; the error is located on the faulty member's line |
| Grammar drift reds (ε) | a kind added to `commonMembers` or `_declaration` without a lowering arm | `grammar_contract_tests` fails, naming the container and the kind |
| Dead arm reds (ε) | a kind removed from the grammar while its arm remains | the same test fails in the other direction |
| Corpus identity (ε, ζ) | `tree-sitter test` and Rust `to_sexp` over tracked `.ri`, before and after | identical; `scripts/tree-sitter-freshness.sh` green |
| Unit op from the CST (ζ) | `5(m)/*c*/*(s)`, `5N·m/rad`, `5kg*m` | `UnitExpr` Mul/Div from the op node's kind; same values as today (`unit_expr_lowering_tests.rs` green) |
| Field source faults (η) | `slc_field_analytical_fault.ri`, `slc_field_imported_fault.ri` | a parse error located at the fault; the compiler's "missing required key: 'format'" is not the only diagnostic |
| Header faults named (η ⊇ #8275) | `slc_struct_header_fault.ri` and #8275's seven cases | one error each, located at the fault, naming the type-parameter list or trait bound |
| Backstop never sole (η, R8) | every probe-table row | at least one error with a kind other than `UnattributedFault` |
| Consumer: CLI | any probe | `reify check` exits non-zero; `line:col: message` |
| Consumer: compiler library | an errored `ParsedModule` passed to `compile_with_prelude_*` | `forward_parse_errors` forwards each error unchanged |
| Consumer: LSP diagnostics | an errored document | `compute_diagnostics_with_state` converts each error and returns before compile |
| Consumer: LSP outline/hover | an errored document with a fault inside one member | outline keeps the container; hover on a sibling member resolves |
| Consumer: module_dag | an imported module whose only error is a backstop error | `compile_project` returns `Err` with `Severity::Error` |
| Close (ω₂) | `ts_parser/*.rs` after the move | all three ratchet rows at 0; `python3 scripts/assert-module-scc.py check` prints no GREW for reify-syntax; `cargo nextest list -p reify-syntax` count equals the pre-move count minus the tests ε, ζ and ω₂ deleted, each named in its commit message |

## §8 Cross-PRD relationship (G4)

| artefact | owner | consumers / other side | this PRD's role | status |
|---|---|---|---|---|
| `crates/reify-syntax/src/ts_parser.rs` lowering dispatch, body loops and fault policy | **this PRD** | S9 γ #8732, μ #8742, S8 π #8904, S9 σ #9322 (one-hunk edits) | owner | file-lock serialised; ω₂ waits on #8904 and #9322 |
| `SpannedIdent` on declared/referenced names | **S9 σ #9322** (fields **and** lowering-site population, R4) | reify-lsp decl_oracle, #7541/#7683/#7684/#8610 | none; ω₂ lands after σ | the G4 row changes from "σ (fields) + syntax-lowering-contract (lowering)" to "σ" (cross-reference line) |
| `ParseErrorKind` (incl. `UnattributedFault`, R8) | **S9 μ #8742** | η, S27 | consumer: η depends on #8742 | requested on #8742 |
| compiler member support (`member_support`, `MemberDecl::tag`) | **S9 γ #8732** | — | adjacent; the vocabularies stay apart (D7); `grammar_contract` is available to γ's B2 | no edge (R5) |
| reify-ast child enumeration | S9 ρ #9321 | S27, #7860 | none | — |
| S23 θ₃ residual sweep (OQ-7 (a)) | S23 | — | ω₂ is a dependency of #9150 | wired |
| reify-audit PLINECITE ledger + `--rename` | S24 ο #8822 | streams that move files | ω₂: symbol cites remove ledger rows 743–746; `--rename` only if #8822 has landed and a surviving cite points at a moved file | same-diff obligation |
| tree-sitter freshness gate | repo gate | — | ε and ζ regenerate | — |
| GUI lezer grammar `gui/src/editor/reify.grammar` | — | — | a third statement of the surface; out of scope (note only) | — |

No reciprocal "the other owns it" remains. σ's description anticipated a lowering leaf from this stream, and
R4 removes that expectation (cross-reference line to the S9 lead).

## §9 Decomposition plan

Every leaf edits `crates/reify-syntax/src/ts_parser.rs` and is serialised by its edges. A leaf that adds tests
under `crates/reify-syntax/tests/harness_syntax*/` re-reads the harness root at leaf time, because
`cargo-native-harness-layout` step A may `git mv` it to `<harness>/main.rs`. A leaf that reds a tracked `.ri`
fixes it in the same diff (α's guard). A leaf that moves a file under PLINECITE scope runs
`plinecite-baseline-gen --rename` in the same diff once #8822 has landed.

| label | id | title | prio | depends on | user-observable signal |
|---|---|---|---|---|---|
| **α** | #9343 | Tracked-`.ri` parse guard and the declared must-error list (the corpus sweep) | medium | #7809 | a tracked `.ri` that gains a CST fault, or a parse error, without being declared must-error reds `cargo test -p reify-syntax`, naming the file |
| **β** | #9344 | `fault_backstop` at the parse entry, `debug_assert`, root-ERROR handling, corpus and probe must-error test | **high**, gate #9171 | α | `reify check tests/prd-gate/fixtures/slc_purpose_param_missing.ri` exits 1 with a located parse error (today rc 0) |
| **γ** | #9346 | `lower_guarded`: one refusal policy over leaf nodes; `check_and_lower!`, `lower_function_checked`, `lower_enum`'s guard deleted | medium | β | `slc_annotation_arg_fault.ri` prints `invalid annotation: @foo(1 +)` at the MISSING operand |
| **δ** | #9347 | `lower_member_list` + `MemberListSpec` over every member body; pragmas rejected in guards and specialization bodies, annotations in specialization bodies | medium | γ | `slc_where_pragma_dropped.ri` exits 1 with `pragma '#frobnicate' is not supported in guarded block` (today rc 0, silent) |
| **ε** | #9348 | Grammar contract: `supertypes`, dispatch tables, `grammar_contract`, node-types and grammar.json contract test (absorbs #7676); wrong-node-kind tests deleted | medium | δ, #8182 | adding a kind to `commonMembers` without a lowering arm reds `grammar_contract_tests` naming it; corpus and Rust S-expressions unchanged |
| **ζ** | #9349 | Unit operator from the CST: anonymous `op` alias; `UnitOp`, `classify_unit_op`, `collect_unit_op_comment_spans`, `strip_unit_op_comments` deleted (K2-D6) | medium | #8182 | `reify eval` of a file with `let q = 5(m)/*c*/*(s)` prints the same value as for `5(m)*(s)`; the lowering holds no source slice between operands |
| **η** | #9350 | Message quality: field definitions and config entries, declaration headers, purpose params; the backstop gets `UnattributedFault`; "backstop never sole" ratchet | medium | δ, #8742 | `slc_field_imported_fault.ri` prints a parse error located at `format =`, not only the compiler's "missing required key: 'format'" |
| **ω₁** | #9351 | Close 1/2: freeze pass-1 into `FileScope`; `DiagSink`; the guard combinator over the sink | medium | α–η, #8182 | behaviour-preserving; signal carried by ω₂ (G2 escape hatch: intermediate unlocking ω₂) |
| **ω₂** | #9352 | Close 2/2: `ExprLowering` in `ts_parser/expr.rs`, `literals.rs`, `tests.rs` dissolved into harness_syntax_lowering, three ratchet rows, K2-D7 prose and INV-SF-7 rehoming | medium | ω₁, #8904, #9322 | `python3 tests/infra/adoption_ratchet.py` shows all three `ts-parser-child-*` rows at 0; ts_parser.rs line count recorded in the commit message (heuristic 14) |
| **Ω** | #9353 | PRD close: SHIPPED header, landed ids, AS-AUTHORED freeze | low | α–ω₂ | the committed header |

**Edges onto other streams' tasks:** `#9171 ← β` (survey gate; high), `#9150 ← ω₂` (S23 OQ-7 (a)). Both were
checked against the live #8612 closure.

### Leaf detail

**α — tracked-`.ri` parse guard** (x_finding_part fk-3553b78476b5).

- Add one test in `tests/harness_syntax/` over `git ls-files '*.ri'`, plus `const MUST_ERROR_RI_FIXTURES:
  &[(&str, &str)]` giving (path, reason). For an undeclared file it asserts a clean Rust CST and an empty
  `reify_syntax::parse(...).errors`. For a declared file it asserts a CST fault; after β it also asserts
  non-empty errors.
- The list starts as the 13 tracked files that already report a parse error, plus
  `decl_passes_purpose_body_sub_swallowed.ri` and this PRD's eight CST-faulty `slc_*` fixtures, each a
  `CstFault` entry with a reason. `slc_where_pragma_dropped.ri` and `slc_spec_body_pragma_dropped.ri` have clean
  CSTs. They join the list as `ErrorsOnly` entries only once δ makes them error, in δ's diff.
- `docs/prds/v0_6/fixtures/indexed_sub_instantiation_surface.ri` is an `ErrorsOnly` entry from the start. It
  carries a clean CST and the #5482 interim rejection. It lives in the deprecated `docs/prds/**/fixtures/`
  tier, so PG-DRIFT does not apply; if #6431 moves it, the list follows in #6431's diff.
- Register every `tests/prd-gate/fixtures/*.ri` the list names in `scripts/verify.sh`'s
  `_RUST_COUPLED_RI_FIXTURES` in the same diff (D12, PG-DRIFT). δ does the same for its two entries.
- Depends on #7809, which removes the two examples' faulty `port` lines.
- **Done when** the test is green on main, naming an undeclared faulty file reds it, and
  `tests/infra/test_verify_scope.sh` is green.

**β — backstop** (x_finding_key fk-3553b78476b5; high; gate #9171).

- D1, D2 and §6.1.
- Ports #7810 (the root-ERROR fixture and its pin), #8080's MISSING-param case (safety), #8275's seven header
  cases (safety, located) and #8081's three cases (safety).
- The probe table inlines each source as a string literal and names no fixture path, so PG-DRIFT is not
  triggered. α's guard already holds the committed fixtures. β extends α's declared-entry assertion from "CST
  fault" to "CST fault and non-empty errors".
- **Done when:**
  - every §2.2 fixture with a CST fault exits `reify check` with rc 1 and a parse error at the fault;
  - the corpus and probe test is green, including non-vacuity;
  - the debug assert is present.
- **Invariant established:** INV-SF-7's safety half, enforced at `parse_with_prelude_enums` (redundantly: the
  backstop plus the assert).
- **G7 waiver:** INV-SF-6 `diagnostics-carry-codes`. `ParseError` carries no code until S9 μ #8742 adds
  `ParseErrorKind`. The backstop constructs through the one constructor μ converts, η adopts
  `UnattributedFault`, and no message-substring consumer is added.

**γ — `lower_guarded`** (x_finding_part fk-3553b78476b5).

- D3, D4 and §6.2.
- Every leaf arm of `lower_source_file` and `lower_member` goes through `lower_guarded`. Annotation and pragma
  arms are guarded at top level and in the existing body loops, which closes #8081's judgement: refusing is
  safe because errors gate compilation (`module_dag.rs` early-returns). δ carries the guard into each spec. The
  structure arm stays unguarded.
- Re-pin the three cases D4 names in `check_and_lower_fault_location_tests.rs`. Keep every MISSING-only
  `invalid connect:` case in `check_and_lower_snippet_bound_tests.rs` verbatim.
- Ratchet row `check-and-lower-macro | check_and_lower![(] | crates/reify-syntax/src/ts_parser.rs | 0 | γ #id`.
- **Done when** `macro_rules! check_and_lower` and `fn lower_function_checked` are absent, the re-pinned and
  unchanged shape tests are green, and the §7 γ rows pass.

**δ — member lists** (x_finding_key fk-7173c189d7ea).

- D5, D6, §6.3 and R7′ (annotations in `where` blocks keep attaching; the two pinned tests stay green).
- Subsumes #7809's trait-body arm (its `trait_tests.rs` must stay green), #8080's body ERROR case, and #8732's
  interim purpose-body arm if it has landed.
- Ratchet row `pending-annotations-bookkeeping | let mut pending_annotations | crates/reify-syntax/src/ts_parser.rs |
  <post count, expected 2: lower_source_file and lower_member_list; 4 on main> | δ #id`.
- Add the two clean-CST pragma fixtures to α's list as `ErrorsOnly` and register them in
  `_RUST_COUPLED_RI_FIXTURES` (D12).
- Census row in `docs/legibility/design-invariants.md`'s umbrella table: `lowering dispatch tables /
  MemberListSpec member sets | grammar_contract_tests (ε) | convention-only until ε`.
- Docs-truth:
  - `crates/reify-mcp/src/tools/chunks/guards.md` gains one line saying that pragmas are not accepted inside
    a `where` block (annotations on guarded params are).
  - `crates/reify-mcp/src/tools/chunks/structures.md` gains one line beside the `sub … { … }` example: a
    specialization body takes param assignments and members only, and pragmas and annotations inside it are
    rejected.
  - The exemplar corpus, cheatsheet and discoverability items are N/A: this adds a rejection, not an authoring
    idiom.
- **Done when** the §7 δ rows pass and `lower_member` has no silent `_ => None`.

**ε — grammar contract** (x_finding_key fk-cff74023db39; absorbs #7676).

- D8, D9 and §6.4.
- `serde_json.workspace = true` in `crates/reify-syntax/Cargo.toml` `[dev-dependencies]`.
- The four wrong-node-kind tests (now in `ts_parser/tests.rs`) are deleted.
- The hand-rolled grammar.js lexer helpers #7676 names (`mask_noncode`, `balanced_arg`, `rule_header`,
  `repeat_call_offsets`, `line_spans`, `contains_string_literal`, `contains_symbol_reference`) are deleted, not
  left unused.
- δ's census row in `docs/legibility/design-invariants.md`'s umbrella table flips to `shipped`.
- **Done when** the §7 ε rows pass.

**ζ — unit operator** (x_finding_key fk-50ed9b0483bf; K2-D6).

- §6.5 and D10. Regenerate through `scripts/tree-sitter-generate.sh`.
- The inline tests referencing the deleted fns, now in `ts_parser/tests.rs`, are deleted or re-expressed
  through `parse()`. So are #6567's two pinning tests (`unrecognized_unit_operator_*` in
  `tests/harness_syntax_lowering/unit_expr_lowering_tests.rs`). #6567 is deferred and was replaced by #6952,
  which is done; ζ makes its bug class unreachable by construction. History is cited as #6952, not #6567.
- The value equality in the signal is a no-regression floor (#6952 already fixed the value). What is new is
  the construct: no source slice between operands (`let op_start = left_node.end_byte()` absent).
- **Done when** `fn classify_unit_op` and `fn strip_unit_op_comments` are absent from `crates/reify-syntax/src`,
  the operand slice is gone from `lower_unit_expr`, `unit_expr_lowering_tests.rs` is green, and the corpus is
  unchanged.

**η — message quality** (x_finding_key fk-653f242d7f93).

- The `field_definition` arm is a guarded leaf. `lower_field_source`'s analytical, composed, sampled and
  imported config loops report a faulty entry at its fault instead of skipping it. This is fk-653f's
  `format =` misattribution.
- Header faults get a header-specific message from `lower_type_parameters`, `find_trait_bound_refs`,
  `lower_trait_bound_refs` and `find_trait_refinement_list` (#8275's message half).
- The purpose-param MISSING case gets a purpose-param message (#8080).
- After #8742, the backstop constructs `ParseErrorKind::UnattributedFault`. The probe table asserts that every
  row carries at least one error of another kind (R8).
- If #8742 landed without the variant, despite the R8 request on its record, η adds it: one variant and its
  `diagnostic_code()` mapping, a sentence-sized widening of S9's type. η then returns a cross-reference line for
  the S9 lead.
- **Done when** the §7 η rows pass.

**ω₁ — close 1/2** (x_finding_part fk-258131627845; K2-D5 A).

- D11 and §6.6.
- **Done when** the full reify-syntax suite is green with no test changed, `struct FileScope` and
  `struct DiagSink` exist under `ts_parser/`, and `Lowering` holds no `known_enums`, `namespace_bindings` or
  `entity_bindings` field.

**ω₂ — close 2/2** (x_finding_key fk-258131627845, fk-cdf6477a3b3d; K2-D5 A + K2-D7).

- D11 and §6.6. Dissolve `ts_parser/tests.rs`.
- Replace every hard line cite in reify-syntax production prose with a symbol cite. Delete the task/5392
  merge-surface sentences.
- Move `member_continuation.rs`'s normative clauses to `docs/legibility/design-invariants.md` §INV-SF-7,
  leaving a one-line pointer. Leave its measurement paragraphs to the #7094 and #8300 commit messages.
- Drop the PLINECITE rows the cite fixes retire.
- Add the census row (§6.6).
- **Done when:**
  - all three ratchet rows are at 0;
  - `task/5392` is absent from `crates/reify-syntax/src`;
  - `struct ExprLowering` is in `ts_parser/expr.rs`;
  - `assert-module-scc.py check` reports no GREW;
  - the heuristic-14 measurement is in the commit message.

**Ω — PRD close.** This leaf follows the overlay's "PRD terminal status" rule: SHIPPED header, landed leaf ids,
AS-AUTHORED freeze and LIVE map, and the same header on the `.capability-manifest.md`.

## §10 Finding-key disposition

| key | leaf |
|---|---|
| fk-3553b78476b5 (high) | β (key); α, γ (parts) |
| fk-653f242d7f93 | η |
| fk-7173c189d7ea | δ |
| fk-cff74023db39 | ε |
| fk-258131627845 | ω₂ (key); ω₁ (part); #8182 (part, K2-D5 B) |
| fk-50ed9b0483bf | ζ |
| fk-cdf6477a3b3d | ω₂ |

## §11 Consolidation map (R2)

| task | safety half | message half | disposition |
|---|---|---|---|
| #7809 | lands as the interim (its own ERROR arm, the example corrections, the breadcrumb) | δ re-expresses the trait body as a spec and keeps `trait_tests.rs` green | stays live; α and δ depend on it |
| #8080 | β (`purpose P(x: ) { }`) | δ (body ERROR → `diagnose_error_node`), η (param message) | cancelled 2026-10-18, `x_subsumed_by` δ #9347 |
| #8081 | β (all three cases) | γ (guarded annotation and pragma, located) | cancelled 2026-10-18, `x_subsumed_by` γ #9346 |
| #8275 | β (seven header cases, located) | η (header-specific message) | cancelled 2026-10-18, `x_subsumed_by` β #9344 |
| #7810 | β (D2) | β | cancelled 2026-10-18, `x_subsumed_by` β #9344 |
| #7676 | — | ε (D9, R3) | cancelled 2026-10-18, `x_subsumed_by` ε #9348 |

Each cancelled task's measured cases, controls and notes are copied into the details of its subsuming leaf (or
leaves) at filing.

## §12 Out of scope

- **Pragmas inside `where` blocks and specialization bodies, and annotations inside specialization bodies, as
  a feature.** These are rejected here (R7′). The decision whether to support them is a pending milestone that
  depends on #8612 and on δ. It is filed by this session as the out-of-plan follow-up #9354. Supporting them needs reify-ast fields under S9's G4 row.
- **The GUI lezer grammar** (`gui/src/editor/reify.grammar`): a third statement of the surface, deliberately
  over-permissive for trait bodies (esc-7809-5).
- **`SpannedIdent` fields and their population** (S9 σ #9322, R4).
- **ParseError codes:** S9 μ #8742. This PRD consumes them.
- **A per-fault (rather than per-child) backstop.** That is message quality, and η may add it if its ratchet
  needs it. It is not required.

## §13 Open questions (tactical)

1. **Backstop message wording.** `syntax error in purpose declaration` (humanised kind) or a `DECLARATION_LOWERERS`
   label once ε lands. Suggested: humanised kind in β, the table label in ε. Decide in β.
2. **`lower_guarded` signature after ω₁.** It could be a `DiagSink` method or a free function over `&DiagSink`.
   Either way the name `lower_guarded` stays (§6.2). Decide in ω₁.
3. **α's guard for the GUI fixture dir.** `gui/test/fixtures/broken_syntax.ri` is a must-error fixture. List it
   or exclude `gui/` with a reason. Decide in α.
