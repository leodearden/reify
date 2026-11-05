//! The shared cross-cfg `GeometryKernel` contract suite (`reify_test_support::assert_kernel_contract!`),
//! real arm, instantiated against `OcctKernelHandle`.

#![cfg(has_occt)]

use reify_ir::{GeometryOp, Value};
use reify_kernel_occt::OcctKernelHandle;

// Shared cross-cfg `GeometryKernel` contract (INV-GEO-4) against the
// real actor-backed kernel, so the has_occt merge gate actually runs
// the OCCT arm of the suite. Previously only reify-test-support's own
// self-test (against a fake `TestRealKernel`) exercised this arm; the
// genuine OCCT kernel was never driven through it. `OcctKernelHandle`
// is used (not the inner single-thread `OcctKernel`) because the
// macro's generated tests upcast to `Box<dyn GeometryKernel>` and the
// handle is the `Send + Sync` type that implements the trait. Each
// generated test spawns its own handle (and OCCT worker thread) for
// isolation; the resulting startup cost is an accepted tradeoff, not
// a correctness concern.
//
// The `stub;` arm (`not(has_occt)`) is deliberately not instantiated
// here: the stub OCCT adapter's all-error taxonomy is already covered
// by bespoke hand-written tests in stubs.rs, and migrating it onto
// this shared suite is tracked as a separate follow-up (#5110) —
// out of scope for this real-arm-only wiring.
reify_test_support::assert_kernel_contract!(
    real;
    OcctKernelHandle::spawn,
    valid_op = GeometryOp::Box {
        size_x: Value::Real(10.0),
        size_y: Value::Real(10.0),
        size_z: Value::Real(10.0),
    },
);
