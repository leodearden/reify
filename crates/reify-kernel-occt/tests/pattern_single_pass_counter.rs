//! Deterministic boolean-op-pass counter tests (task 5213).
//!
//! The single-pass n-ary fuse converts each pattern realizer from N−1 pairwise
//! `BRepAlgoAPI_Fuse` builds into exactly ONE boolean pass.  A wall-clock
//! threshold would be flaky and environment-sensitive, so instead we assert the
//! *mechanism* directly: a process-global atomic counter, incremented once per
//! completed OCCT boolean `Build()`, lets a test observe exactly how many
//! boolean passes an operation performed.
//!
//! `reify_kernel_occt::reset_boolean_pass_count()` zeroes the counter and
//! `reify_kernel_occt::boolean_pass_count()` reads it.
//!
//! ## Serialization
//!
//! The counter is process-global, so parallel test threads in THIS binary would
//! corrupt each other's reset→operate→read windows.  Every counter-touching
//! test here runs under `with_counter_lock`, which holds a single process-wide
//! mutex across the whole window.  This file is the SOLE reader of the counter;
//! other integration-test files run as separate processes (separate statics),
//! so no cross-file interference is possible.
//!
//! `#![cfg(has_occt)]` gates the file to hosts with OCCT (the standard verify
//! environment always sets it).

#![cfg(has_occt)]

use std::sync::Mutex;

use reify_ir::{GeometryHandleId, GeometryOp, Value};
use reify_kernel_occt::{boolean_pass_count, reset_boolean_pass_count, OcctKernel};

/// Process-wide guard serializing all counter reset→operate→read windows in
/// this binary.  A plain `()` payload — it exists only for mutual exclusion.
static COUNTER_LOCK: Mutex<()> = Mutex::new(());

/// Run `f` while holding the counter lock, recovering from poisoning so one
/// panicking assertion does not cascade into misleading failures elsewhere.
fn with_counter_lock<T>(f: impl FnOnce() -> T) -> T {
    let _guard = COUNTER_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    f()
}

/// Build a fresh 1 m unit cube centred at the origin; return its handle id.
fn unit_box(kernel: &mut OcctKernel) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(1.0),
            height: Value::Real(1.0),
            depth: Value::Real(1.0),
        })
        .expect("unit box creation should succeed")
        .id
}

/// Translate `src` by `dx` metres along +X, returning a fresh handle id.
fn translated_x(kernel: &mut OcctKernel, src: GeometryHandleId, dx: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Translate {
            target: src,
            dx,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("translate should succeed")
        .id
}

/// A single binary boolean (Union → `boolean_fuse`) performs exactly ONE pass.
#[test]
fn binary_union_is_one_boolean_pass() {
    with_counter_lock(|| {
        let mut kernel = OcctKernel::new();
        let a = unit_box(&mut kernel);
        let b = translated_x(&mut kernel, a, 0.5); // overlapping → a real fuse

        reset_boolean_pass_count();
        kernel
            .execute(&GeometryOp::Union { left: a, right: b })
            .expect("binary union must succeed");
        assert_eq!(
            boolean_pass_count(),
            1,
            "a single binary Union must perform exactly 1 boolean pass"
        );
    });
}

/// `fuse_all` of K=5 shapes performs exactly ONE pass — single-pass, NOT K−1.
#[test]
fn fuse_all_of_five_is_one_boolean_pass() {
    with_counter_lock(|| {
        let mut kernel = OcctKernel::new();
        // Five disjoint unit boxes at x = 0, 2, 4, 6, 8.
        let b0 = unit_box(&mut kernel);
        let ids = [
            b0,
            translated_x(&mut kernel, b0, 2.0),
            translated_x(&mut kernel, b0, 4.0),
            translated_x(&mut kernel, b0, 6.0),
            translated_x(&mut kernel, b0, 8.0),
        ];

        reset_boolean_pass_count();
        kernel
            .fuse_all(&ids)
            .expect("fuse_all of five boxes must succeed");
        assert_eq!(
            boolean_pass_count(),
            1,
            "fuse_all of K=5 shapes must be a single boolean pass, not K-1"
        );
    });
}
