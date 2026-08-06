//! Deterministic boolean-op-pass counter tests (task 5213).
//!
//! The single-pass n-ary fuse converts each pattern realizer from N−1 pairwise
//! `BRepAlgoAPI_Fuse` builds into exactly ONE boolean pass.  A wall-clock
//! threshold would be flaky and environment-sensitive, so instead we assert the
//! *mechanism* directly: a counter incremented once per completed OCCT boolean
//! `Build()` lets a test observe exactly how many boolean passes an operation
//! performed.
//!
//! `reify_kernel_occt::reset_boolean_pass_count()` zeroes the counter and
//! `reify_kernel_occt::boolean_pass_count()` reads it.
//!
//! ## Isolation
//!
//! The counter is thread-local and libtest runs each `#[test]` on its own
//! thread, so every reset→operate→read window here is isolated by construction
//! — under plain `cargo test`, under `--test-threads=1` (one thread running
//! tests sequentially, where windows cannot interleave at all), and under
//! nextest's process-per-test alike.  No mutex is needed and no
//! `--test-threads=1` requirement applies.
//!
//! `#![cfg(has_occt)]` gates the file to hosts with OCCT (the standard verify
//! environment always sets it).

#![cfg(has_occt)]

use reify_ir::{GeometryHandleId, GeometryOp, Value};
use reify_kernel_occt::{boolean_pass_count, reset_boolean_pass_count, OcctKernel};

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
}

/// `fuse_all` of K=5 shapes performs exactly ONE pass — single-pass, NOT K−1.
#[test]
fn fuse_all_of_five_is_one_boolean_pass() {
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
}

/// A 3×3 `LinearPattern2D` (9 instances) must perform exactly ONE boolean pass,
/// not the 8 pairwise fuses the accumulator loop did.
#[test]
fn linear_pattern_2d_3x3_is_one_pass() {
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
}

/// A 1×1 `LinearPattern2D` is a single instance: NO boolean pass at all.
#[test]
fn linear_pattern_2d_1x1_is_zero_passes() {
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
}

/// A 1-D `LinearPattern` of count 3 (≥2 instances) must be exactly ONE pass.
#[test]
fn linear_pattern_1d_is_one_pass() {
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
}

/// A `CircularPattern` of count 4 (≥2 instances) must be exactly ONE pass.
#[test]
fn circular_pattern_is_one_pass() {
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
}

/// Boolean passes performed on ANOTHER thread must not leak INTO this thread's
/// open reset→operate→read window: the counter is per-thread, so this thread
/// observes only the single pass it performed itself.
#[test]
fn boolean_work_on_another_thread_stays_out_of_this_threads_window() {
    let mut kernel = OcctKernel::new();
    let a = unit_box(&mut kernel);
    let b = translated_x(&mut kernel, a, 0.5); // overlapping → a real fuse

    reset_boolean_pass_count();

    // Three real boolean passes on a FOREIGN thread, performed entirely inside
    // this thread's now-open window.  `OcctKernel` is `!Send` (it holds
    // `cxx::UniquePtr`), so the worker's kernel is constructed INSIDE the
    // closure and never captured across the thread boundary.
    //
    // The `join()` before this thread touches OCCT again is load-bearing, not
    // incidental: src/handle.rs documents concurrent OCCT access from multiple
    // threads as undefined behaviour (which is why `OcctKernelHandle` exists),
    // so the two threads must never be inside OCCT at the same time.  A
    // sequential handoff still reproduces the defect exactly — the failure is a
    // foreign increment landing inside this thread's open window, and
    // sequencing changes nothing about that.
    std::thread::spawn(|| {
        let mut worker_kernel = OcctKernel::new();
        let wa = unit_box(&mut worker_kernel);
        let wb = translated_x(&mut worker_kernel, wa, 0.5);
        for _ in 0..3 {
            worker_kernel
                .execute(&GeometryOp::Union {
                    left: wa,
                    right: wb,
                })
                .expect("worker-thread union must succeed");
        }
    })
    .join()
    .expect("worker thread must not panic");

    kernel
        .execute(&GeometryOp::Union { left: a, right: b })
        .expect("binary union must succeed");
    assert_eq!(
        boolean_pass_count(),
        1,
        "the boolean-pass count is per-thread: this thread performed exactly 1 \
         pass, so the 3 passes performed on the worker thread must not be \
         visible from here"
    );
}

/// The per-thread contract holds in the other direction too: passes performed
/// on THIS thread must not leak OUT into a freshly spawned thread's count.
#[test]
fn a_freshly_spawned_thread_starts_from_a_zero_count() {
    let mut kernel = OcctKernel::new();
    let a = unit_box(&mut kernel);
    let b = translated_x(&mut kernel, a, 0.5); // overlapping → a real fuse

    reset_boolean_pass_count();
    kernel
        .execute(&GeometryOp::Union { left: a, right: b })
        .expect("binary union must succeed");
    // This thread now stands at exactly 1.

    // The reader thread performs no OCCT work at all — it only reads the
    // counter — so this test adds no concurrent-OCCT exposure.
    let observed = std::thread::spawn(boolean_pass_count)
        .join()
        .expect("reader thread must not panic");
    assert_eq!(
        observed, 0,
        "a freshly spawned thread starts from its own zero count, so the 1 \
         boolean pass performed on the test thread must not be visible there"
    );
}

/// An `ArbitraryPattern` with 2 transforms (≥2 instances) must be ONE pass.
#[test]
fn arbitrary_pattern_is_one_pass() {
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
}
