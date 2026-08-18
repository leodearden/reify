//! Consolidated integration-test harness for the module/namespace and
//! ports/entity-composition subsystems.
//!
//! Task #5693 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, leaf CMP-3,
//! batch 3 of 6): folds 24 former standalone `tests/*.rs` binaries into this single
//! compile unit to cut the merge-gate link count. Layout-only — no `#[test]` fn is
//! added or removed. Each former file is included as a stem-named module so its
//! `<file>::<test>` module path (and thus every `test(/^<file>::/)` filterset)
//! resolves unchanged.
//!
//! Explicit `#[path]` is MANDATORY: this harness root is an integration-test crate
//! root, where a bare `mod <file>;` would resolve to the sibling `tests/<file>.rs`,
//! not the `harness_modules_ports/` subdir — and mid-move, while both spellings
//! exist on disk, it would SILENTLY bind the stale top-level file instead of failing.
//!
//! The shared `common` helper module is declared ONCE here at the harness root (via
//! `#[path = "common/mod.rs"]` — note no `../`, this root is a sibling of
//! `tests/common/`); the four former `mod common;`-using members now reach it through
//! `crate::common`. Declaring it per-file would load the same source several times in
//! this one compile unit, which `clippy::duplicate_mod` rejects under `-D warnings`.
//!
//! The cluster boundary is the §11 architect call: the module/namespace and
//! ports/entity-composition clusters are merged into ONE unit precisely so that all
//! four `tests/common/mod.rs` consumers sit here, and the sibling
//! `harness_compilation_surface` unit pays no external `common` charge at all
//! (`harness_layout_unit_lines` charges every out-of-module-dir include to EACH
//! including unit, so a split would pay those 363 lines twice).
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
