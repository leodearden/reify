//! Consolidated integration-test harness for the language-core subsystem
//! (type / let / priv / parametric / specialization / spec).
//!
//! Task #5283 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, leaf CMP-1,
//! batch 1 of 2): folds the former standalone `tests/{type,let,priv,parametric,
//! specialization,spec}_*.rs` binaries into this single compile unit to cut the
//! merge-gate link count. Layout-only — no `#[test]` fn is added or removed. Each former
//! file is included as a stem-named module so its `<file>::<test>` module path (and thus
//! every `test(/^<file>::/)` filterset) resolves unchanged. Explicit `#[path]` is required:
//! this harness root is an integration-test crate root, where a bare `mod <file>;` would
//! resolve to the sibling `tests/<file>.rs`, not the `harness_langcore/` subdir. The shared
//! `common` helper module is declared ONCE here at the harness root (via
//! `#[path = "common/mod.rs"]`); the three former `mod common;`-using parametric files now
//! import it as `use crate::common::…` (declaring it per-file would load the same source
//! multiple times in this one compile unit, which `clippy::duplicate_mod` rejects). The two
//! `include_str!` fixture consumers climb to `../fixtures/` from the subdir.
#[path = "common/mod.rs"]
mod common;

#[path = "harness_langcore/let_annotation_type_mismatch_tests.rs"]
mod let_annotation_type_mismatch_tests;
#[path = "harness_langcore/let_scope_tests.rs"]
mod let_scope_tests;
#[path = "harness_langcore/let_type_disambiguation_tests.rs"]
mod let_type_disambiguation_tests;
#[path = "harness_langcore/parametric_alias_def_site_validation_tests.rs"]
mod parametric_alias_def_site_validation_tests;
#[path = "harness_langcore/parametric_field_resolution_tests.rs"]
mod parametric_field_resolution_tests;
#[path = "harness_langcore/parametric_tensor_resolution_tests.rs"]
mod parametric_tensor_resolution_tests;
#[path = "harness_langcore/parametric_vector_point_resolution_tests.rs"]
mod parametric_vector_point_resolution_tests;
#[path = "harness_langcore/priv_import_boundary_tests.rs"]
mod priv_import_boundary_tests;
#[path = "harness_langcore/priv_member_visibility_tests.rs"]
mod priv_member_visibility_tests;
#[path = "harness_langcore/priv_redundant_tests.rs"]
mod priv_redundant_tests;
#[path = "harness_langcore/spec_param_override_compile_tests.rs"]
mod spec_param_override_compile_tests;
#[path = "harness_langcore/specialization_scope_e2e_tests.rs"]
mod specialization_scope_e2e_tests;
#[path = "harness_langcore/specialization_scope_lowering_tests.rs"]
mod specialization_scope_lowering_tests;
#[path = "harness_langcore/specialization_scope_validation_tests.rs"]
mod specialization_scope_validation_tests;
#[path = "harness_langcore/type_alias_compile_tests.rs"]
mod type_alias_compile_tests;
#[path = "harness_langcore/type_arg_applied_resolution_tests.rs"]
mod type_arg_applied_resolution_tests;
#[path = "harness_langcore/type_error_propagation_tests.rs"]
mod type_error_propagation_tests;
#[path = "harness_langcore/type_expr_kind_dispatch_tests.rs"]
mod type_expr_kind_dispatch_tests;
#[path = "harness_langcore/type_hygiene_integration_gate.rs"]
mod type_hygiene_integration_gate;
