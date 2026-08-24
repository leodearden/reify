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
//!
//! Task #6082 lands `euler_convention_arg_types.rs` here rather than as a top-level
//! standalone `tests/euler_convention_arg_types.rs` (which
//! scripts/check-harness-baseline-registration.sh flags
//! `reason=unregistered-standalone`; the sanctioned remedy is consolidation, NOT a new
//! tests/infra/harness-layout-baseline.manifest grandfather row — SUPERSEDED, Leo
//! 2026-07-22, esc-5056-11: the baseline is a shrinking ratchet, not an allow-list to
//! grow). It belongs to this subsystem for the same reason the `fn_enum_param_resolution_tests`
//! / `fn_enum_return_resolution_tests` / `fn_signature_type_resolution_tests` modules do:
//! it pins a callable's declared argument and return types — here the two Euler builtins'
//! `EulerConvention` argument slot and their declared result types. Its `include_str!`
//! fixture consumers climb to `../fixtures/` from the subdir.
#[path = "harness_traits/euler_convention_arg_types.rs"]
mod euler_convention_arg_types;
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
