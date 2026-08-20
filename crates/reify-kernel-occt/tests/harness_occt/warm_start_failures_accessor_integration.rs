//! INV-GEO-3 (kernel-seam λ, task 5112): `OcctKernel::warm_start_failures()`
//! must be a production accessor — callable from a normal (non-`cfg(test)`)
//! build — not a dev-only diagnostic.
//!
//! Integration tests compile the lib WITHOUT `--cfg test`, so this is the
//! only way to prove the accessor lives on the production
//! `#[cfg(has_occt)] impl OcctKernel` block rather than the
//! `#[cfg(all(test, has_occt))]` one: on the latter, calling
//! `warm_start_failures()` from here is a compile-time `E0599` error, not a
//! runtime failure.
//!
//! `OcctWarmState` is crate-private (`reify-kernel-occt/src/lib.rs`, no
//! `pub`), so this test drives the warm-start round-trip through the public
//! `reify_ir::WarmStartable` trait rather than constructing the state
//! directly — see `tests/topology_cache_observability.rs` for the same
//! `#![cfg(has_occt)]` + box-kernel pattern.

#![cfg(has_occt)]

use reify_ir::{GeometryOp, Value, WarmStartable};
use reify_kernel_occt::OcctKernel;

/// A fully-valid `warm_state()` → `with_warm_state()` round-trip must report
/// zero deserialization failures, and the count must be readable via
/// `warm_start_failures()` from a production (non-`cfg(test)`) build.
#[test]
fn warm_start_failures_accessor_visible_in_production_build() {
    let mut source = OcctKernel::new();
    source
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("Box creation should succeed");
    let state = source
        .warm_state()
        .expect("a kernel holding a shape should produce warm state");

    let mut restored = OcctKernel::new();
    restored.with_warm_state(state);

    assert_eq!(
        restored.warm_start_failures(),
        0,
        "an all-valid warm-state round-trip should report zero deserialization failures"
    );
}
