//! Consolidated integration-test harness for the compilation-surface subsystem —
//! what you hand the compiler (compile API/builder, examples, pragmas) plus the
//! declaration-level metadata that rides along (purpose, meta, doc, test markers).
//!
//! Task #5693 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, leaf CMP-3,
//! batch 3 of 6): folds 8 former standalone `tests/*.rs` binaries into this single
//! compile unit to cut the merge-gate link count. Layout-only — no `#[test]` fn is
//! added or removed. Each former file is included as a stem-named module so its
//! `<file>::<test>` module path (and thus every `test(/^<file>::/)` filterset)
//! resolves unchanged.
//!
//! Explicit `#[path]` is MANDATORY: this harness root is an integration-test crate
//! root, where a bare `mod <file>;` would resolve to the sibling `tests/<file>.rs`,
//! not the `harness_compilation_surface/` subdir — and mid-move, while both
//! spellings exist on disk, it would SILENTLY bind the stale top-level file instead
//! of failing.
//!
//! This unit declares NO `common`, and that is a designed property rather than an
//! oversight: all four `tests/common/mod.rs` consumers in this batch were routed
//! into the sibling `harness_modules_ports` unit, so this unit's `external_lines`
//! is 0 by construction. Do NOT add a `#[path = "common/mod.rs"] mod common;` here
//! — `harness_layout_unit_lines` charges an out-of-module-dir include to EACH
//! including unit, so an unused one would both bill 363 external lines against the
//! cap and trip an unused-module warning. CMP-2 had to amend exactly such a stray
//! include out of harness_geometry_solver (2d479616b2).
#[path = "harness_compilation_surface/compile_api_tests.rs"]
mod compile_api_tests;
#[path = "harness_compilation_surface/compile_builder_smoke_tests.rs"]
mod compile_builder_smoke_tests;
#[path = "harness_compilation_surface/doc_propagation_tests.rs"]
mod doc_propagation_tests;
#[path = "harness_compilation_surface/examples_smoke.rs"]
mod examples_smoke;
#[path = "harness_compilation_surface/meta_compile_tests.rs"]
mod meta_compile_tests;
#[path = "harness_compilation_surface/pragma_compile_tests.rs"]
mod pragma_compile_tests;
#[path = "harness_compilation_surface/purpose_compile_tests.rs"]
mod purpose_compile_tests;
#[path = "harness_compilation_surface/test_marker_compile_tests.rs"]
mod test_marker_compile_tests;
