//! Consolidated integration-test harness for reify-syntax's CST->AST lowering tests.
//!
//! Split out of `harness_syntax.rs` for HARNESS_KLOC_CAP rule (a) headroom
//! (task #7040). Layout-only — no `#[test]` fn is added or removed, and every
//! `<stem>::<test>` module path resolves unchanged. Explicit `#[path]` is
//! mandatory here (PRD docs/prds/merge-gate-compile-cost.md §5 C1): a harness
//! root is an integration-test crate root, where a bare `mod <stem>;` would
//! resolve against `tests/`, not the `harness_syntax_lowering/` subdir.
//!
//! Module order: alphabetical by stem.
#[path = "harness_syntax_lowering/auto_binding_sites_lowering_tests.rs"]
mod auto_binding_sites_lowering_tests;
#[path = "harness_syntax_lowering/aux_at_lowering_tests.rs"]
mod aux_at_lowering_tests;
#[path = "harness_syntax_lowering/enum_named_field_lowering_tests.rs"]
mod enum_named_field_lowering_tests;
#[path = "harness_syntax_lowering/enum_type_param_lowering_tests.rs"]
mod enum_type_param_lowering_tests;
#[path = "harness_syntax_lowering/imaginary_literal_lowering_tests.rs"]
mod imaginary_literal_lowering_tests;
#[path = "harness_syntax_lowering/joint_with_lowering_tests.rs"]
mod joint_with_lowering_tests;
#[path = "harness_syntax_lowering/namespaced_ref_lowering_tests.rs"]
mod namespaced_ref_lowering_tests;
#[path = "harness_syntax_lowering/numeric_separators_lowering_tests.rs"]
mod numeric_separators_lowering_tests;
#[path = "harness_syntax_lowering/radix_literals_lowering_tests.rs"]
mod radix_literals_lowering_tests;
#[path = "harness_syntax_lowering/relate_at_auto_lowering_tests.rs"]
mod relate_at_auto_lowering_tests;
#[path = "harness_syntax_lowering/trait_assoc_fn_call_lowering_tests.rs"]
mod trait_assoc_fn_call_lowering_tests;
#[path = "harness_syntax_lowering/trait_assoc_fn_member_lowering_tests.rs"]
mod trait_assoc_fn_member_lowering_tests;
#[path = "harness_syntax_lowering/unit_expr_lowering_tests.rs"]
mod unit_expr_lowering_tests;
#[path = "harness_syntax_lowering/value_pow_lowering_tests.rs"]
mod value_pow_lowering_tests;
