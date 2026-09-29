//! Consolidated integration-test harness for the STATIC TYPE-CHECKING subsystem —
//! whether an expression type-checks and what the checker says when it does not:
//! per-operator operand guards (add/sub, mul/div, and/or, comparison, chained
//! comparison, implies, mod, pow), builtin argument-signature resolution, receiver and
//! member-access typing, `self` / datum projection, literal and sentinel typing
//! (`undef`, polymorphic zero, dimensionless-real unification), and the static-vs-runtime
//! parity of the arithmetic the checker admits.
//!
//! Task #5695 (PRD `docs/prds/merge-gate-compile-cost.md` §3 W1 / §5 C1, leaf CMP-5,
//! batch 5 of 6): folds 20 former standalone `tests/*.rs` binaries into this single
//! compile unit to cut the merge-gate link count.
//!
//! Layout contract C1 — stem-named modules, why the `#[path]` on every declaration
//! below is mandatory rather than stylistic, the kLOC cap, the baseline ratchet, and
//! the by-design test-id change (no `#[test]` fn added or removed, invariant I3):
//! see `tests/infra/test_harness_kloc_cap.sh`'s C1/C2 header, kept there, not
//! restated here.
//!
//! Crate-local: this harness declares the shared `common` helper ONCE, because a
//! per-member `mod common;` would load the same source repeatedly in one compile unit
//! (`clippy::duplicate_mod`). Its two consumers (`value_mod_compile_tests`,
//! `value_pow_compile_tests`) reach it as `use crate::common[::…]` — so this unit's
//! `external_lines` charge is that one shared `tests/common/mod.rs`, expected rather
//! than stray. The line count itself is computed by `harness_layout_unit_lines`
//! (tests/infra/harness-layout-lib.sh) and deliberately not pinned here, where nothing
//! would recompute it.
//!
//! `mul_div_static_runtime_parity` carries its own `#![cfg(feature = "test-support")]`
//! inner attribute. It belongs to that member and must stay inside it: hoisting it here
//! would silently gate all 20 members on the feature.
//!
//! Routing vs. the two sibling roots added by this same leaf — the line is subsystem,
//! not filename. THIS root asks whether an EXPRESSION type-checks.
//! `harness_structure_declarations` holds the user-declared structure shapes and their
//! construction-time conformance; `harness_statement_semantics` holds what a STATEMENT
//! means once its expressions check. Against the pre-existing neighbours:
//! `harness_traits` owns callable conformance and trait/assoc-type resolution,
//! `harness_constructor_typing` the builtin constructor-family return-type ladder, and
//! `harness_units_materials` the unit MACHINERY behind dimension mismatches — the
//! operand guards here assert the diagnostic, not the unit algebra that produces it.
//! `unresolved_function_tests` (task #5371) is here on the same subsystem line: it
//! pins what the checker says, and deliberately what it still TYPES, when a callee
//! no ladder arm claims reaches `expr.rs`'s terminal first-arg fallback — an
//! expression-type-checking question, sibling to `builtin_arg_signature_tests`. The
//! corpus-wide sweep that same task added is NOT here: compiling every committed
//! `.ri` is a compilation-surface question, so it sits beside `examples_smoke` in
//! `harness_compilation_surface`.
#[path = "common/mod.rs"]
mod common;
#[path = "harness_type_checking/add_sub_operand_guard_tests.rs"]
mod add_sub_operand_guard_tests;
#[path = "harness_type_checking/and_or_operand_guard_tests.rs"]
mod and_or_operand_guard_tests;
#[path = "harness_type_checking/boolean_arg_cross_sub_diagnostic_tests.rs"]
mod boolean_arg_cross_sub_diagnostic_tests;
#[path = "harness_type_checking/builtin_arg_signature_tests.rs"]
mod builtin_arg_signature_tests;
#[path = "harness_type_checking/chained_comparison_tests.rs"]
mod chained_comparison_tests;
#[path = "harness_type_checking/comparison_operand_guard_tests.rs"]
mod comparison_operand_guard_tests;
#[path = "harness_type_checking/expr_error_sentinel_tests.rs"]
mod expr_error_sentinel_tests;
#[path = "harness_type_checking/field_op_typing_tests.rs"]
mod field_op_typing_tests;
#[path = "harness_type_checking/implies_type_check_tests.rs"]
mod implies_type_check_tests;
#[path = "harness_type_checking/mul_div_operand_guard_tests.rs"]
mod mul_div_operand_guard_tests;
#[path = "harness_type_checking/mul_div_static_runtime_parity.rs"]
mod mul_div_static_runtime_parity;
#[path = "harness_type_checking/numeric_and_range_literals_example.rs"]
mod numeric_and_range_literals_example;
#[path = "harness_type_checking/polymorphic_zero_tests.rs"]
mod polymorphic_zero_tests;
#[path = "harness_type_checking/real_dimensionless_unification_tests.rs"]
mod real_dimensionless_unification_tests;
#[path = "harness_type_checking/self_datum_projection_tests.rs"]
mod self_datum_projection_tests;
#[path = "harness_type_checking/self_keyword_tests.rs"]
mod self_keyword_tests;
#[path = "harness_type_checking/undef_literal_compile_tests.rs"]
mod undef_literal_compile_tests;
#[path = "harness_type_checking/unresolved_function_tests.rs"]
mod unresolved_function_tests;
#[path = "harness_type_checking/value_mod_compile_tests.rs"]
mod value_mod_compile_tests;
#[path = "harness_type_checking/value_pow_compile_tests.rs"]
mod value_pow_compile_tests;
#[path = "harness_type_checking/wrong_receiver_member_tests.rs"]
mod wrong_receiver_member_tests;
