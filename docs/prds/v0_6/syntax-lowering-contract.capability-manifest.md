# Capability manifest — syntax-lowering-contract

PRD: `docs/prds/v0_6/syntax-lowering-contract.md`. Machine-readable twin:
`docs/prds/v0_6/syntax-lowering-contract.capability-manifest.yaml` (stamped by `commit_planning`). Evidence
measured on main `baff3ed38b` (2026-10-17), using a debug `reify` built on 2026-10-16 at 14:45; no commit has
touched the parser paths since. Grammar premises were probed in a scratch copy of `tree-sitter-reify` (CLI
0.26.8) and with a scratch Rust prober on tree-sitter 0.24.7, the production runtime. The fixtures are in
`tests/prd-gate/fixtures/`. Every delivered check below was measured to FAIL on main and to PASS once its
producer lands.

**Decompose verification.** `scripts/prd-decompose-verify.mjs` ran on 2026-10-18 (run `wf_0066c884-b8a`) over
α, β, δ, ε, ζ and η. γ, ω₁, ω₂ and Ω assert only code structure, which the greps below bind. The workflow's
disposition was BLOCKS, with 6/6 leaves probed and 0 malformed records. Its blocking items resolve as follows:

- **Two real findings, fixed in the PRD before filing:**
  - α: a clean-CST file with errors, now an `ErrorsOnly` entry.
  - δ: the trailing-annotation row was wrong, and annotations in `where` blocks are attached, not dropped
    (ruled R7′ by Leo).
- **Producer-self postconditions probed against today's tree:** β's and δ's "after the leaf" rejections. They
  fail today by construction.
- **Probe-binding defects in the harness, not premise falsifications.** Each premise passes when bound
  correctly, which the workflow's own records show:
  - ε's supertypes premise was bound against a corpus `.txt` file and the unedited grammar;
  - β's `stderr_pattern` premise uses a key the α runner ignores.

Verdict key: **PASS** means the binding held at authoring time. "producer-self" means the capability is the
leaf's own deliverable.

## α — Tracked-`.ri` parse guard and declared must-error list

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| tracked-ri-silent-set-measured | Of 867 tracked `.ri`, 16 have `root.has_error()` in the Rust runtime. 13 already print a parse error. Three are silent (rc 0): `examples/m9_trait_conformance.ri`, `examples/integration_corner_cases.ri`, `tests/prd-gate/fixtures/decl_passes_purpose_body_sub_swallowed.ri` | PASS | — |
| clean-cst-errors-only-entry | Of the 851 tracked `.ri` with a clean CST, exactly one prints a parse error: `docs/prds/v0_6/fixtures/indexed_sub_instantiation_surface.ri` (#5482's interim rejection). Declared as `ErrorsOnly` (decompose-verify workflow, 2026-10-18) | PASS | — |
| examples-corrected-upstream | producer: #7809, upstream (esc-7809-5 corrections on `task/7809`). The two examples are evaluated by `m9_trait_conformance.rs` and `eval_invariant_corpus_sweep.rs` | PASS | — |
| must-error-list | producer-self | PASS | grep `MUST_ERROR_RI_FIXTURES` present in `crates/reify-syntax/tests` |

## β — Backstop (high, gate #9171)

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| silent-accepts-on-main | `reify check` gives rc 0 and "All constraints satisfied." on `slc_purpose_param_missing.ri`, `slc_purpose_body_error.ri`, `slc_trait_body_port.ri`, `slc_struct_header_fault.ri`, `slc_annotation_arg_fault.ri` (which prints only "unknown annotation @foo") and `slc_field_analytical_fault.ri`. All have CST faults. Today's missing rejection is the motivation; β adds it | PASS | — |
| no-module-level-check-today | `parse_with_prelude_enums` lowers, then calls `check_member_continuations`, then returns | PASS | — |
| consumers-gate-on-errors | The CLI, the GUI engine and the LSP diagnostics all return before compiling when there are errors (doc of `compile_builder/pre_pass.rs::forward_parse_errors`) | PASS | — |
| corpus-fault-entries | 8 of 263 corpus entries expect ERROR/MISSING. Corpus trees are CLI output, so the test checks the Rust tree first | PASS | — |
| root-error-fixture | `slc_root_error.ri` has root = ERROR in the Rust runtime and gives rc 1 with eight errors, including `unexpected '<kind>' in source file` | PASS | — |
| backstop-fn | producer-self | PASS | grep `fn fault_backstop` present in `ts_parser/fault_diagnosis.rs` |
| use-site-assert | producer-self | PASS | grep `debug_assert![(]!root[.]has_error` present in `ts_parser.rs` |
| probes-loud | behavioural: covered by the probe-table test | PASS | manual |

## γ — `lower_guarded`

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| three-mechanisms-on-main | `check_and_lower!`, `lower_function_checked`, `lower_enum` / `lower_type_args_from_node` | PASS | — |
| message-strings-unpinned | No test pins `syntax error in function definition` or `syntax error in enum declaration`. The `invalid <label>:` shape survives where no inner arm reports. γ re-pins three cases to the inner message: port-body ERROR, connect-body ERROR and the guarded-block let (D4) | PASS | — |
| combinator | producer-self. The name is kept through ω₁; `lower_guarded_block` does not match the pattern | PASS | grep `fn lower_guarded[<(]` present in `crates/reify-syntax/src` |
| macro-gone | producer-self | PASS | grep `macro_rules! check_and_lower` absent |
| fn-checked-gone | producer-self | PASS | grep `fn lower_function_checked` absent |

## δ — Member lists

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| silent-pragma-drops-on-main | `slc_where_pragma_dropped.ri` and `slc_spec_body_pragma_dropped.ri` have clean CSTs and give rc 0 with no diagnostic. The control in a structure body warns "unknown pragma" | PASS | — |
| grammar-admits-them | node-types.json lists annotation and pragma under `guarded_block` and `specialization_body`. There is no reify-ast field to hold them, hence Reject (R7′) | PASS | — |
| where-annotations-kept | Annotations in `where` blocks attach today, pinned by `solver_hint_in_guarded_block_compiles` and `display_wrong_ladder_on_guarded_param_errors`. Both stay green (R7′) | PASS | — |
| spec-type | producer-self | PASS | grep `struct MemberListSpec` present |
| reject-policy-used | producer-self (R7) | PASS | grep `PragmaPolicy::Reject` present |
| trait-body-interim-kept | producer: #7809, upstream | PASS | — |

## ε — Grammar contract (absorbs #7676)

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| supertypes-accepted | In the scratch copy all four supertypes generate. The corpus passes 263/263, and Rust `to_sexp` is identical over the 867 tracked `.ri`. node-types.json gains four `subtypes` entries | PASS | — |
| node-types-present-at-test-time | `tree-sitter-reify/build.rs` regenerates `node-types.json` and `grammar.json` (both gitignored) before the test links | PASS | — |
| supertypes-declared | producer-self | PASS | grep `supertypes:` present in `grammar.js` |
| contract-surface | producer-self | PASS | grep `pub mod grammar_contract` present in `lib.rs` |
| hand-lexer-gone | producer-self (#7676) | PASS | grep `fn mask_noncode` absent in `crates/reify-syntax/tests` |
| drift-reds | behavioural | PASS | manual |

## ζ — Unit operator from the CST (K2-D6)

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| alias-premise | Generation is clean, the corpus is unchanged and Rust `to_sexp` is identical. The `op` field becomes visible as an anonymous `*` or `/` node; `·` is `*` with text `·`. The comment is excluded from the op node. `parser.c` changes by 8 lines | PASS | — |
| alias-declared | producer-self | PASS | grep `alias[(][$][.]_unit_mul_op` present in `grammar.js` |
| classifier-gone | producer-self | PASS | grep `fn classify_unit_op[(]` absent |
| stripper-gone | producer-self | PASS | grep `fn strip_unit_op_comments[<(]` absent |
| no-operand-slice | producer-self. The construct is bound, so the value equality is only a no-regression floor | PASS | grep `op_start = left_node[.]end_byte` absent |

## η — Message quality

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| field-faults-on-main | `slc_field_analytical_fault.ri` gives rc 0 silently. `slc_field_imported_fault.ri` gives rc 1, but only through two compiler errors ("missing required key: 'format'" and "… 'grid'") | PASS | — |
| kind-variant-upstream | producer: #8742 (S9 μ), upstream; `UnattributedFault` requested under R8 | PASS | — |
| backstop-kind | producer-self | PASS | grep `UnattributedFault` present in `crates/reify-syntax/src` |
| never-sole | behavioural | PASS | manual |

## ω₁ — Close 1/2

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| pass-one-tables-mutable | The three pass-1 tables are mutable `Lowering` fields written only in pass 1. The expression partition is all `&self` | PASS | — |
| file-scope | producer-self | PASS | grep `struct FileScope` present |
| diag-sink | producer-self | PASS | grep `struct DiagSink` present |

## ω₂ — Close 2/2

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| ratchet-mechanism | `tests/infra/adoption-ratchet.manifest` rows are anchored at column 0, so a nested `mod tests { use super::*; }` does not count | PASS | — |
| expr-lowering | producer-self | PASS | grep `struct ExprLowering` present |
| ratchet-rows | producer-self: three rows (`impl-lowering`, `lowering-import`, `glob-import`) | PASS | grep `ts-parser-child-impl-lowering` present in the manifest |
| merge-surface-prose-gone | producer-self (K2-D7); 2 hits on main | PASS | grep `task/5392` absent in `crates/reify-syntax/src` |
| upstream-one-hunk-editors | producers: #8904 and #9322, upstream | PASS | — |

## Ω — PRD close

| capability | evidence | verdict | delivered check |
|---|---|---|---|
| terminal-stamp | overlay "PRD terminal status" | PASS | grep `^\*\*Status:\*\* \*\*SHIPPED` present in the PRD |
