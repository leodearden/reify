//! Consolidated integration-test harness for the DECLARATION-CHECKING subsystem —
//! the user-declared data shapes and whether a declaration is well-formed: structures
//! and their fields, collections, options and option recovery/resolution, constants,
//! recursive-shape detection, constructor-field conformance and constructor hashing,
//! ambient structures in a purpose, and declaration-name shadowing.
//!
//! Task #5695 (PRD `docs/prds/merge-gate-compile-cost.md` §3 W1 / §5 C1, leaf CMP-5,
//! batch 5 of 6): folds 15 former standalone `tests/*.rs` binaries into this single
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
//! (`clippy::duplicate_mod`). Its one consumer, `struct_ctor_field_conformance_tests`,
//! reaches it as `use crate::common[::…]` — so this unit's `external_lines` charge is
//! that one shared `tests/common/mod.rs`, expected rather than stray. The line count
//! itself is computed by `harness_layout_unit_lines`
//! (tests/infra/harness-layout-lib.sh) and deliberately not pinned here, where nothing
//! would recompute it.
//!
//! Routing, recorded so it is not re-litigated: this root is NOT `harness_langcore`
//! (type alias, `let`, `priv`, parametric declarations, specialization) and NOT
//! `harness_patterns` (match / enum). It holds the user-declared STRUCTURE shapes and
//! their construction-time conformance. `harness_langcore` could not have absorbed this
//! group in any case — measure both with `harness_layout_unit_lines` and their sum is
//! comfortably over the C2 hard cap, so the choice was never between two viable homes.
//! Against the two sibling roots added by this same leaf: `harness_type_checking` asks
//! whether an EXPRESSION type-checks, `harness_statement_semantics` what a STATEMENT
//! means; this root asks whether a DECLARATION is well-formed and whether a value
//! constructed from it conforms.
#[path = "common/mod.rs"]
mod common;
#[path = "harness_structure_declarations/collection_compile_tests.rs"]
mod collection_compile_tests;
#[path = "harness_structure_declarations/collection_sub_tests.rs"]
mod collection_sub_tests;
#[path = "harness_structure_declarations/constant_compile_tests.rs"]
mod constant_compile_tests;
#[path = "harness_structure_declarations/constants_example_tests.rs"]
mod constants_example_tests;
#[path = "harness_structure_declarations/constructor_hash_tests.rs"]
mod constructor_hash_tests;
#[path = "harness_structure_declarations/field_compile_tests.rs"]
mod field_compile_tests;
#[path = "harness_structure_declarations/fields_stdlib_compile.rs"]
mod fields_stdlib_compile;
#[path = "harness_structure_declarations/option_compile_tests.rs"]
mod option_compile_tests;
#[path = "harness_structure_declarations/option_recovery_resolution_tests.rs"]
mod option_recovery_resolution_tests;
#[path = "harness_structure_declarations/recursive_detection_tests.rs"]
mod recursive_detection_tests;
#[path = "harness_structure_declarations/recursive_structure_tests.rs"]
mod recursive_structure_tests;
#[path = "harness_structure_declarations/same_module_structure_ctor_compile.rs"]
mod same_module_structure_ctor_compile;
#[path = "harness_structure_declarations/shadowing_warning_tests.rs"]
mod shadowing_warning_tests;
#[path = "harness_structure_declarations/struct_ctor_field_conformance_tests.rs"]
mod struct_ctor_field_conformance_tests;
#[path = "harness_structure_declarations/structure_in_purpose_ambient_tests.rs"]
mod structure_in_purpose_ambient_tests;
