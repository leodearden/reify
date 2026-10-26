//! Tests of the `OcctKernel` / `OcctKernelHandle` public surface, moved out of the in-file
//! test modules of `src/lib.rs` and `src/handle.rs` by #8648
//! (PRD docs/prds/v0_6/occt-crate-internals.md D10).
//!
//! Membership rule: a member drives only pub methods, the `test-fixtures` `*_for_test`
//! constructors and `reify_ir`. Like `harness_occt`, no member uses `tests/common/`, so this
//! root declares no `mod common;`. Every member carries `#![cfg(has_occt)]` and this root is
//! deliberately ungated, for the same reason `harness_occt.rs` gives. The layout contract
//! lives in `tests/infra/test_harness_kloc_cap.sh`'s C1 header.

#[path = "harness_occt_kernel/handle.rs"]
mod handle;
#[path = "harness_occt_kernel/handle_step_export.rs"]
mod handle_step_export;
#[path = "harness_occt_kernel/input_validation.rs"]
mod input_validation;
#[path = "harness_occt_kernel/kernel_contract.rs"]
mod kernel_contract;
#[path = "harness_occt_kernel/lifecycle.rs"]
mod lifecycle;
#[path = "harness_occt_kernel/modify_ops.rs"]
mod modify_ops;
#[path = "harness_occt_kernel/support.rs"]
mod support;
#[path = "harness_occt_kernel/sweeps.rs"]
mod sweeps;
#[path = "harness_occt_kernel/transforms_patterns.rs"]
mod transforms_patterns;
