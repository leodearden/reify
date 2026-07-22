//! Consolidated integration-test harness for the pattern-matching subsystem
//! (match / enum).
//!
//! Task #5283 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, leaf CMP-1,
//! batch 1 of 2): folds the former standalone `tests/match_*.rs` and `tests/enum_*.rs`
//! binaries into this single compile unit to cut the merge-gate link count. Layout-only —
//! no `#[test]` fn is added or removed. Each former file is included as a stem-named module
//! so its `<file>::<test>` module path (and thus every `test(/^<file>::/)` filterset)
//! resolves unchanged. Explicit `#[path]` is required: this harness root is an
//! integration-test crate root, where a bare `mod <file>;` would resolve to the sibling
//! `tests/<file>.rs`, not the `harness_patterns/` subdir. The three `mod common;`-using enum
//! files load the shared helper via `#[path = "../common/mod.rs"]`.
#[path = "harness_patterns/enum_generic_construction_inference_tests.rs"]
mod enum_generic_construction_inference_tests;
#[path = "harness_patterns/enum_generic_ir_lowering_tests.rs"]
mod enum_generic_ir_lowering_tests;
#[path = "harness_patterns/enum_pattern_field_check_tests.rs"]
mod enum_pattern_field_check_tests;
#[path = "harness_patterns/enum_unknown_type_param_tests.rs"]
mod enum_unknown_type_param_tests;
#[path = "harness_patterns/match_arm_decl_group_compile_tests.rs"]
mod match_arm_decl_group_compile_tests;
#[path = "harness_patterns/match_arm_decl_group_typing_tests.rs"]
mod match_arm_decl_group_typing_tests;
#[path = "harness_patterns/match_block_decl_lowering_tests.rs"]
mod match_block_decl_lowering_tests;
#[path = "harness_patterns/match_compile_tests.rs"]
mod match_compile_tests;
