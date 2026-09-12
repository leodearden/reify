//! Consolidated integration-test harness for the module/namespace and
//! ports/entity-composition subsystems.
//!
//! Task #5693 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, leaf CMP-3,
//! batch 3 of 6): folds 24 former standalone `tests/*.rs` binaries into this single
//! compile unit to cut the merge-gate link count. Layout-only — no `#[test]` fn is
//! added or removed.
//!
//! The layout contract those `#[path]` lines below obey — why `#[path]` is MANDATORY
//! in a harness root rather than stylistic, what naming each module for its former
//! file stem does and does not preserve about a test id, and how an include that
//! escapes the module dir is charged to EACH including unit — is stated ONCE, in
//! `tests/infra/test_harness_kloc_cap.sh`'s C1/C2 header, and mechanically enforced
//! by that guard's §6/§7. Deliberately not restated here: a restated contract is a
//! copy that can drift from the one the guard actually enforces.
//!
//! Two facts specific to THIS unit:
//!   * It declares `common`, exactly once, here at the root (`#[path = "common/mod.rs"]`
//!     — note no `../`: the root is a sibling of `tests/common/`). Its four
//!     `common`-using members reach it as `crate::common`; a per-file `mod common;`
//!     would load that source several times within this one compile unit, which
//!     `clippy::duplicate_mod` rejects under `-D warnings`.
//!   * Merging the module/namespace and ports/entity-composition clusters into one
//!     unit is the §11 architect call, made precisely so that all four
//!     `tests/common/mod.rs` consumers land HERE — which is what lets the sibling
//!     `harness_compilation_surface` unit carry no external charge at all, paying
//!     `common`'s 363 lines once instead of twice.
#[path = "common/mod.rs"]
mod common;

#[path = "harness_modules_ports/alias_dfs_diagnostic_tests.rs"]
mod alias_dfs_diagnostic_tests;
#[path = "harness_modules_ports/connect_compile_tests.rs"]
mod connect_compile_tests;
#[path = "harness_modules_ports/deprecated_use_tests.rs"]
mod deprecated_use_tests;
#[path = "harness_modules_ports/entity_overload_tests.rs"]
mod entity_overload_tests;
#[path = "harness_modules_ports/import_resolve_tests.rs"]
mod import_resolve_tests;
#[path = "harness_modules_ports/import_warning_tests.rs"]
mod import_warning_tests;
#[path = "harness_modules_ports/io_traits_tests.rs"]
mod io_traits_tests;
#[path = "harness_modules_ports/module_dag_tests.rs"]
mod module_dag_tests;
#[path = "harness_modules_ports/occurrence_compile_tests.rs"]
mod occurrence_compile_tests;
#[path = "harness_modules_ports/port_compile_tests.rs"]
mod port_compile_tests;
#[path = "harness_modules_ports/ports_prelude_test.rs"]
mod ports_prelude_test;
#[path = "harness_modules_ports/ports_stdlib_compile.rs"]
mod ports_stdlib_compile;
#[path = "harness_modules_ports/prelude_context_tests.rs"]
mod prelude_context_tests;
#[path = "harness_modules_ports/qualified_access_compile_tests.rs"]
mod qualified_access_compile_tests;
#[path = "harness_modules_ports/reserved_name_lint_tests.rs"]
mod reserved_name_lint_tests;
#[path = "harness_modules_ports/standard_gravity_tests.rs"]
mod standard_gravity_tests;
#[path = "harness_modules_ports/standard_joint_library_tests.rs"]
mod standard_joint_library_tests;
#[path = "harness_modules_ports/standard_stock_tests.rs"]
mod standard_stock_tests;
#[path = "harness_modules_ports/sub_placement_lowering_tests.rs"]
mod sub_placement_lowering_tests;
#[path = "harness_modules_ports/sub_structure_existence_tests.rs"]
mod sub_structure_existence_tests;
#[path = "harness_modules_ports/subbody_objective_ignored_tests.rs"]
mod subbody_objective_ignored_tests;
#[path = "harness_modules_ports/task_1570_tests.rs"]
mod task_1570_tests;
#[path = "harness_modules_ports/user_defined_unit_tests.rs"]
mod user_defined_unit_tests;
#[path = "harness_modules_ports/visibility_compile_tests.rs"]
mod visibility_compile_tests;
