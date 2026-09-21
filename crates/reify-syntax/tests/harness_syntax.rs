//! Consolidated integration-test harness for reify-syntax's parser/grammar tests.
//!
//! Task #5275 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, leaf C-syntax)
//! folded the former 61 standalone `tests/<file>.rs` binaries into one compile unit to cut
//! the merge-gate link count. Task #7040 then split the CST→AST lowering family out into
//! the sibling root `harness_syntax_lowering.rs`, so this is no longer reify-syntax's
//! single test binary: it now holds the 49 grammar/parser-side modules below, and its
//! sibling holds 14. Layout-only in both directions — no `#[test]` fn was added or removed
//! by either. Each former file is included as a stem-named module so its `<file>::<test>`
//! module path (and thus every `test(/^<file>::/)` filterset) resolves unchanged.
//! Explicit `#[path]` is required: this harness root is an integration-test crate root,
//! where a bare `mod <file>;` would resolve to the sibling `tests/<file>.rs`, not the
//! `harness_syntax/` subdir — mirroring crates/reify-eval/tests/harness_geometry.rs.
//!
//! WHY THE LOWERING FAMILY LEFT. This unit was nearing
//! `tests/infra/test_harness_kloc_cap.sh`'s rule (a) cap with `module_lines` dominating the
//! breakdown, which §7 of the PRD resolves by SPLIT rather than by raising the cap.
//! `harness_syntax_lowering.rs` carries the full rationale and the one copy of the measured
//! before/after for both units; the guard re-derives the live numbers on demand, so they are
//! deliberately not restated here.
//!
//! `common` (the shared tree-sitter CST helper module under `tests/common/`) is declared
//! exactly once here, at the crate root, rather than once per dependent submodule. Ten of
//! the modules below previously each carried their own `#[path = "../common/mod.rs"] mod
//! common;` (correct when each was its own standalone binary/crate root); folded together
//! as sibling submodules of one crate, those ten declarations all loaded the same physical
//! file, which `clippy::duplicate_mod` (denied via `-D warnings`) rejects. Declaring it once
//! here and having dependents `use crate::common::{...}` preserves the single shared
//! implementation without a duplicate load; `common` carries no `#[test]` fns, so this does
//! not affect any `<file>::<test>` module path.
//!
//! The split did not change where `common` is charged, and must not: it is compiled into
//! THIS binary and no other, and `harness_syntax_lowering.rs` declares no `mod common;`.
//! See that root for why a second declaration would work against a cap-relief split.
#[path = "common/mod.rs"]
mod common;
#[path = "harness_syntax/ad_hoc_selector_tests.rs"]
mod ad_hoc_selector_tests;
#[path = "harness_syntax/affine_map_spec_example_parses.rs"]
mod affine_map_spec_example_parses;
#[path = "harness_syntax/annotation_tests.rs"]
mod annotation_tests;
#[path = "harness_syntax/assoc_type_consumption_tests.rs"]
mod assoc_type_consumption_tests;
#[path = "harness_syntax/auto_binding_sites_grammar_tests.rs"]
mod auto_binding_sites_grammar_tests;
#[path = "harness_syntax/auto_type_arg_tests.rs"]
mod auto_type_arg_tests;
#[path = "harness_syntax/boundary1_producer.rs"]
mod boundary1_producer;
#[path = "harness_syntax/cfg_import_attachment_tests.rs"]
mod cfg_import_attachment_tests;
#[path = "harness_syntax/check_and_lower_snippet_bound_tests.rs"]
mod check_and_lower_snippet_bound_tests;
#[path = "harness_syntax/connect_chain_tests.rs"]
mod connect_chain_tests;
#[path = "harness_syntax/constraint_def_tests.rs"]
mod constraint_def_tests;
#[path = "harness_syntax/constraint_inst_tests.rs"]
mod constraint_inst_tests;
#[path = "harness_syntax/default_decl_tests.rs"]
mod default_decl_tests;
#[path = "harness_syntax/derived_sub_arm_parser_tests.rs"]
mod derived_sub_arm_parser_tests;
#[path = "harness_syntax/edge_case_tests.rs"]
mod edge_case_tests;
#[path = "harness_syntax/field_tests.rs"]
mod field_tests;
#[path = "harness_syntax/fn_body_expr_parser_tests.rs"]
mod fn_body_expr_parser_tests;
#[path = "harness_syntax/fn_body_separator_ambiguity_tests.rs"]
mod fn_body_separator_ambiguity_tests;
#[path = "harness_syntax/fn_param_default_tests.rs"]
mod fn_param_default_tests;
#[path = "harness_syntax/forall_statement_tests.rs"]
mod forall_statement_tests;
#[path = "harness_syntax/function_call_named_args_tests.rs"]
mod function_call_named_args_tests;
#[path = "harness_syntax/guard_tests.rs"]
mod guard_tests;
#[path = "harness_syntax/import_tests.rs"]
mod import_tests;
#[path = "harness_syntax/indexed_sub_instantiation_parser_tests.rs"]
mod indexed_sub_instantiation_parser_tests;
#[path = "harness_syntax/interpolated_string_tests.rs"]
mod interpolated_string_tests;
#[path = "harness_syntax/keyed_sub_member_block_parser_tests.rs"]
mod keyed_sub_member_block_parser_tests;
#[path = "harness_syntax/lambda_tests.rs"]
mod lambda_tests;
#[path = "harness_syntax/match_decl_block_parser_tests.rs"]
mod match_decl_block_parser_tests;
#[path = "harness_syntax/match_decl_block_tests.rs"]
mod match_decl_block_tests;
#[path = "harness_syntax/match_tests.rs"]
mod match_tests;
#[path = "harness_syntax/member_continuation_ambiguity_tests.rs"]
mod member_continuation_ambiguity_tests;
#[path = "harness_syntax/member_span_tests.rs"]
mod member_span_tests;
#[path = "harness_syntax/module_decl_tests.rs"]
mod module_decl_tests;
#[path = "harness_syntax/numeric_separators_grammar_tests.rs"]
mod numeric_separators_grammar_tests;
#[path = "harness_syntax/occurrence_tests.rs"]
mod occurrence_tests;
#[path = "harness_syntax/option_tests.rs"]
mod option_tests;
#[path = "harness_syntax/port_tests.rs"]
mod port_tests;
#[path = "harness_syntax/pragma_tests.rs"]
mod pragma_tests;
#[path = "harness_syntax/priv_modifier_tests.rs"]
mod priv_modifier_tests;
#[path = "harness_syntax/purpose_tests.rs"]
mod purpose_tests;
#[path = "harness_syntax/qualified_access_tests.rs"]
mod qualified_access_tests;
#[path = "harness_syntax/quantifier_tests.rs"]
mod quantifier_tests;
#[path = "harness_syntax/radix_literals_grammar_tests.rs"]
mod radix_literals_grammar_tests;
#[path = "harness_syntax/scientific_notation_tests.rs"]
mod scientific_notation_tests;
#[path = "harness_syntax/sub_decl_specialization_body_parser_tests.rs"]
mod sub_decl_specialization_body_parser_tests;
#[path = "harness_syntax/sub_decl_specialization_tests.rs"]
mod sub_decl_specialization_tests;
#[path = "harness_syntax/sub_placement_spec_example_parses.rs"]
mod sub_placement_spec_example_parses;
#[path = "harness_syntax/trait_tests.rs"]
mod trait_tests;
#[path = "harness_syntax/type_alias_tests.rs"]
mod type_alias_tests;
#[path = "harness_syntax/type_expr_kind_tests.rs"]
mod type_expr_kind_tests;
#[path = "harness_syntax/undef_literal_tests.rs"]
mod undef_literal_tests;
#[path = "harness_syntax/unit_decl_tests.rs"]
mod unit_decl_tests;
#[path = "harness_syntax/visibility_tests.rs"]
mod visibility_tests;
