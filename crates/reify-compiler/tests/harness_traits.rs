//! Consolidated integration-test harness for the traits / callable-conformance subsystem.
//!
//! Task #5283 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, leaf CMP-1,
//! batch 1 of 2): folds the former standalone `tests/trait_*.rs` and `tests/fn_*.rs`
//! binaries into this single compile unit to cut the merge-gate link count. Layout-only —
//! no `#[test]` fn is added or removed. Each former file is included as a stem-named module
//! so its `<file>::<test>` module path (and thus every `test(/^<file>::/)` filterset)
//! resolves unchanged. Explicit `#[path]` is required: this harness root is an
//! integration-test crate root, where a bare `mod <file>;` would resolve to the sibling
//! `tests/<file>.rs`, not the `harness_traits/` subdir.
#[path = "harness_traits/fn_arg_trait_conformance_tests.rs"]
mod fn_arg_trait_conformance_tests;
#[path = "harness_traits/fn_enum_param_resolution_tests.rs"]
mod fn_enum_param_resolution_tests;
#[path = "harness_traits/fn_enum_return_resolution_tests.rs"]
mod fn_enum_return_resolution_tests;
#[path = "harness_traits/fn_generic_body_permissive_tests.rs"]
mod fn_generic_body_permissive_tests;
#[path = "harness_traits/fn_generic_call_inference_tests.rs"]
mod fn_generic_call_inference_tests;
#[path = "harness_traits/fn_generic_signature_tests.rs"]
mod fn_generic_signature_tests;
#[path = "harness_traits/fn_generic_trait_bound_tests.rs"]
mod fn_generic_trait_bound_tests;
#[path = "harness_traits/fn_overload_tests.rs"]
mod fn_overload_tests;
#[path = "harness_traits/fn_param_default_consumption_tests.rs"]
mod fn_param_default_consumption_tests;
#[path = "harness_traits/fn_param_struct_ctor_default_tests.rs"]
mod fn_param_struct_ctor_default_tests;
#[path = "harness_traits/fn_signature_type_resolution_tests.rs"]
mod fn_signature_type_resolution_tests;
#[path = "harness_traits/trait_arg_conformance_bench.rs"]
mod trait_arg_conformance_bench;
#[path = "harness_traits/trait_assoc_fn_conformance_tests.rs"]
mod trait_assoc_fn_conformance_tests;
#[path = "harness_traits/trait_assoc_fn_instance_tests.rs"]
mod trait_assoc_fn_instance_tests;
#[path = "harness_traits/trait_assoc_fn_overload_tests.rs"]
mod trait_assoc_fn_overload_tests;
#[path = "harness_traits/trait_assoc_fn_static_tests.rs"]
mod trait_assoc_fn_static_tests;
#[path = "harness_traits/trait_assoc_fn_structure_override_tests.rs"]
mod trait_assoc_fn_structure_override_tests;
#[path = "harness_traits/trait_assoc_type_conformance_tests.rs"]
mod trait_assoc_type_conformance_tests;
#[path = "harness_traits/trait_assoc_type_qualified_resolution_tests.rs"]
mod trait_assoc_type_qualified_resolution_tests;
#[path = "harness_traits/trait_assoc_type_resolution_tests.rs"]
mod trait_assoc_type_resolution_tests;
#[path = "harness_traits/trait_bounds_tests.rs"]
mod trait_bounds_tests;
#[path = "harness_traits/trait_conformance_tests.rs"]
mod trait_conformance_tests;
#[path = "harness_traits/trait_conformance_type_error_tests.rs"]
mod trait_conformance_type_error_tests;
#[path = "harness_traits/trait_default_collision_tests.rs"]
mod trait_default_collision_tests;
#[path = "harness_traits/trait_merge_tests.rs"]
mod trait_merge_tests;
#[path = "harness_traits/trait_type_arg_rejection_tests.rs"]
mod trait_type_arg_rejection_tests;
#[path = "harness_traits/trait_typed_param_tests.rs"]
mod trait_typed_param_tests;
