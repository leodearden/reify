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

/// A 3×3 `LinearPattern2D` (9 instances) must perform exactly ONE boolean pass,
/// not the 8 pairwise fuses the accumulator loop did.
#[test]
fn linear_pattern_2d_3x3_is_one_pass() {
    with_counter_lock(|| {
        let mut kernel = OcctKernel::new();
        let b = unit_box(&mut kernel);

        reset_boolean_pass_count();
        kernel
            .execute(&GeometryOp::LinearPattern2D {
                target: b,
                direction1: [1.0, 0.0, 0.0],
                count1: 3,
                spacing1: Value::Real(2.0),
                direction2: [0.0, 1.0, 0.0],
                count2: 3,
                spacing2: Value::Real(2.0),
            })
            .expect("3x3 linear_pattern_2d must succeed");
        assert_eq!(
            boolean_pass_count(),
            1,
            "a 9-instance LinearPattern2D must be exactly 1 boolean pass, not 8"
        );
    });
}

/// A 1×1 `LinearPattern2D` is a single instance: NO boolean pass at all.
#[test]
fn linear_pattern_2d_1x1_is_zero_passes() {
    with_counter_lock(|| {
        let mut kernel = OcctKernel::new();
        let b = unit_box(&mut kernel);

        reset_boolean_pass_count();
        kernel
            .execute(&GeometryOp::LinearPattern2D {
                target: b,
                direction1: [1.0, 0.0, 0.0],
                count1: 1,
                spacing1: Value::Real(2.0),
                direction2: [0.0, 1.0, 0.0],
                count2: 1,
                spacing2: Value::Real(2.0),
            })
            .expect("1x1 linear_pattern_2d must succeed");
        assert_eq!(
            boolean_pass_count(),
            0,
            "a single-instance pattern must perform 0 boolean passes (short-circuit)"
        );
    });
}

/// A 1-D `LinearPattern` of count 3 (≥2 instances) must be exactly ONE pass.
#[test]
fn linear_pattern_1d_is_one_pass() {
    with_counter_lock(|| {
        let mut kernel = OcctKernel::new();
        let b = unit_box(&mut kernel);

        reset_boolean_pass_count();
        kernel
            .execute(&GeometryOp::LinearPattern {
                target: b,
                direction: [1.0, 0.0, 0.0],
                count: 3,
                spacing: Value::Real(2.0),
            })
            .expect("linear_pattern of count 3 must succeed");
        assert_eq!(
            boolean_pass_count(),
            1,
            "a 3-instance LinearPattern must be exactly 1 boolean pass, not 2"
        );
    });
}

/// A `CircularPattern` of count 4 (≥2 instances) must be exactly ONE pass.
#[test]
fn circular_pattern_is_one_pass() {
    with_counter_lock(|| {
        let mut kernel = OcctKernel::new();
        // Offset the box from the Z axis so the four rotations are disjoint,
        // distinct instances (a centred cube has 90° symmetry).
        let b = unit_box(&mut kernel);
        let b_off = translated_x(&mut kernel, b, 3.0);

        reset_boolean_pass_count();
        kernel
            .execute(&GeometryOp::CircularPattern {
                target: b_off,
                axis_origin: [0.0, 0.0, 0.0],
                axis_dir: [0.0, 0.0, 1.0],
                count: 4,
                angle: Value::Real(std::f64::consts::TAU),
            })
            .expect("circular_pattern of count 4 must succeed");
        assert_eq!(
            boolean_pass_count(),
            1,
            "a 4-instance CircularPattern must be exactly 1 boolean pass, not 3"
        );
    });
}

/// An `ArbitraryPattern` with 2 transforms (≥2 instances) must be ONE pass.
#[test]
fn arbitrary_pattern_is_one_pass() {
    with_counter_lock(|| {
        let mut kernel = OcctKernel::new();
        let b = unit_box(&mut kernel);

        // Two pure translations (identity quaternion), both away from origin,
        // so instances are distinct: original ∪ copy@3 ∪ copy@6.
        reset_boolean_pass_count();
        kernel
            .execute(&GeometryOp::ArbitraryPattern {
                target: b,
                transforms: vec![
                    ([1.0, 0.0, 0.0, 0.0], [3.0, 0.0, 0.0]),
                    ([1.0, 0.0, 0.0, 0.0], [6.0, 0.0, 0.0]),
                ],
            })
            .expect("arbitrary_pattern of 2 transforms must succeed");
        assert_eq!(
            boolean_pass_count(),
            1,
            "a ≥2-instance ArbitraryPattern must be exactly 1 boolean pass"
        );
    });
}
