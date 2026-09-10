//! The empty-shape PRECONDITION: which consumers of a shape refuse one that
//! carries no topology, and — just as load-bearing — which deliberately do not
//! (task 5318, ruling of 2026-09-08 / esc-5318-7).
//!
//! An empty OCCT boolean result is a LEGAL kernel value. `BRepAlgoAPI_Common`
//! on disjoint operands, and `BRepAlgoAPI_Cut` whose tool fully consumes its
//! target, report `IsDone() == true` and hand back an empty `TopoDS_Compound`;
//! `harness_occt::boolean_result_normalization_integration::
//! empty_boolean_results_stay_untouched_compounds` gates that, and
//! `examples/tolerancing/gdt_oracle_inside.ri` DESIGNS on it (an empty cut is
//! the "inside" verdict, and `volume()` of it is 0.0). So the guard cannot live
//! at the boolean producer.
//!
//! It lives instead at the ops that MINT AN ARTIFACT from a shape and cannot
//! mint one from nothing: the profile-solidifying sweep family, and STEP
//! export. Those are what this file pins, in one place, together with the
//! exclusions:
//!
//!   * NOT guarded — the booleans (the ruling, and the gate above);
//!     `volume`/`area`/mass-property queries (an empty shape's 0.0 IS the GD&T
//!     oracle's answer channel); tessellation (an empty mesh is an honest
//!     rendering, and `zone_profile_realize_smoke` relies on it); the
//!     transforms (empty in, empty out — the emptiness survives to a real
//!     consumer); `fillet`/`chamfer` (already refused by task 7054's
//!     `BRepKind::Solid` gate, since an empty result classifies as `Compound`);
//!     and `fuse_shape_list` (a pure union over a non-empty list, on the hot
//!     pattern-realizer path).
//!
//! The over-fire controls are as important as the rejections: they are what
//! keeps "the operands do not touch" from being confused with "the result is
//! empty". Their expected volumes are exact closed forms for axis-aligned
//! boxes, not tuned thresholds.

#![cfg(has_occt)]

use reify_ir::{GeometryError, GeometryHandleId, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::OcctKernel;

/// Build an axis-aligned cube of side `side`, centred on the origin
/// (`make_box` centres its output — cpp/occt_wrapper.cpp).
fn cube(kernel: &mut OcctKernel, side: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(side),
            height: Value::Real(side),
            depth: Value::Real(side),
        })
        .expect("box creation should succeed")
        .id
}

/// Translate `target` along X, returning a fresh handle.
fn translated_x(kernel: &mut OcctKernel, target: GeometryHandleId, dx: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Translate {
            target,
            dx,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("translate should succeed")
        .id
}

/// Intersect two disjoint 10-unit cubes, yielding a shape with NO topology.
///
/// This rests on a landed gated fact rather than an assumption: main's
/// `empty_boolean_results_stay_untouched_compounds` already pins that this
/// SUCCEEDS and classifies as `BRepKind::Compound` with zero volume.
fn empty_intersection(kernel: &mut OcctKernel) -> GeometryHandleId {
    let left = cube(kernel, 10.0);
    let right_raw = cube(kernel, 10.0);
    let right = translated_x(kernel, right_raw, 100.0);
    kernel
        .execute(&GeometryOp::Intersection { left, right })
        .expect("an intersection of disjoint operands must SUCCEED with an empty result")
        .id
}

/// A real 5-unit-radius disk face, the ordinary profile the sweep family takes.
fn circle_profile(kernel: &mut OcctKernel, radius: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::CircleProfile {
            radius: Value::Real(radius),
        })
        .expect("CircleProfile creation should succeed")
        .id
}

fn volume_of(kernel: &OcctKernel, id: GeometryHandleId) -> f64 {
    match kernel.query(&GeometryQuery::Volume(id)) {
        Ok(Value::Real(v)) => v,
        other => panic!("expected Value::Real from a Volume query, got {other:?}"),
    }
}

/// Assert `id`'s volume is within `rel_tol` relative of `expected` (non-zero).
fn assert_volume_near(kernel: &OcctKernel, id: GeometryHandleId, expected: f64, what: &str) {
    let v = volume_of(kernel, id);
    let rel = (v - expected).abs() / expected.abs();
    assert!(
        rel < 1e-6,
        "{what}: volume {v:.9} not within 1e-6 relative of the exact closed form \
         {expected:.9} (rel={rel:.3e})"
    );
}

/// Assert `result` is `Err(GeometryError::OperationFailed(msg))` naming the
/// input as `empty` and naming the argument role the guard reports
/// (`role_word`, e.g. "profile"), so a designer reading it knows WHICH of their
/// arguments collapsed.
///
/// Substring matching is deliberately loose so re-wording the diagnostic does
/// not make these tests brittle. Generic over the `Ok` type so one assertion
/// covers `execute(..)` (a `GeometryHandle`) and the with-history entry points
/// (a `(GeometryHandle, …Records)` pair).
fn assert_empty_input_rejected<T>(result: Result<T, GeometryError>, role_word: &str) {
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("empty"),
                "expected a message naming the input as empty, got: {msg}"
            );
            assert!(
                msg.contains(role_word),
                "expected a message naming '{role_word}', got: {msg}"
            );
        }
        Ok(_) => panic!("expected OperationFailed for an empty '{role_word}', got Ok"),
        Err(other) => {
            panic!("expected OperationFailed for an empty '{role_word}', got {other:?}")
        }
    }
}

// --- Extrude: an empty profile cannot be solidified ---

/// The direct `OcctKernel::execute` path, which reaches `make_prism`
/// (src/lib.rs `GeometryOp::Extrude` -> `ffi::ffi::make_prism`). Today this
/// validates only the distance scalar and hands the empty shape straight to
/// `BRepPrimAPI_MakePrism`.
#[test]
fn execute_extrude_of_an_empty_profile_is_rejected() {
    let mut kernel = OcctKernel::new();
    let empty = empty_intersection(&mut kernel);

    let result = kernel.execute(&GeometryOp::Extrude {
        profile: empty,
        distance: Value::Real(0.010),
    });
    assert_empty_input_rejected(result, "profile");
}

/// The PRODUCTION path: `handle.rs` routes `GeometryOp::Extrude` through
/// `extrude_with_history`, so a guard that covered only `execute` would leave
/// every real build unprotected.
#[test]
fn extrude_with_history_of_an_empty_profile_is_rejected() {
    let mut kernel = OcctKernel::new();
    let empty = empty_intersection(&mut kernel);

    let result = kernel.extrude_with_history(empty, 0.010);
    assert_empty_input_rejected(result, "profile");
}

/// `make_prism_infinite` is a separate FFI entry point with its own validation,
/// so it needs its own call site and its own pin.
#[test]
fn execute_extrude_infinite_of_an_empty_profile_is_rejected() {
    let mut kernel = OcctKernel::new();
    let empty = empty_intersection(&mut kernel);

    let result = kernel.execute(&GeometryOp::ExtrudeInfinite {
        profile: empty,
        axis: [0.0, 0.0, 1.0],
        both: false,
    });
    assert_empty_input_rejected(result, "profile");
}

// --- OVER-FIRE CONTROLS ---

/// The ordinary case the guard must leave alone: a real disk face extrudes to
/// the exact cylinder volume pi*r^2*h = pi*0.005^2*0.010.
#[test]
fn execute_extrude_of_a_real_face_still_succeeds() {
    let mut kernel = OcctKernel::new();
    let profile = circle_profile(&mut kernel, 0.005);

    let handle = kernel
        .execute(&GeometryOp::Extrude {
            profile,
            distance: Value::Real(0.010),
        })
        .expect("extruding a real face must succeed");
    assert_volume_near(
        &kernel,
        handle.id,
        std::f64::consts::PI * 0.005_f64.powi(2) * 0.010,
        "extrude of a real disk face",
    );
}

/// Two axis-aligned 10-cubes offset 5 on X overlap in exactly 5*10*10 = 500.
#[test]
fn execute_intersection_of_overlapping_boxes_still_succeeds() {
    let mut kernel = OcctKernel::new();
    let left = cube(&mut kernel, 10.0);
    let right_raw = cube(&mut kernel, 10.0);
    let right = translated_x(&mut kernel, right_raw, 5.0);

    let handle = kernel
        .execute(&GeometryOp::Intersection { left, right })
        .expect("overlapping intersection must succeed");
    assert_volume_near(&kernel, handle.id, 500.0, "overlapping intersection");
}

/// A tool that misses entirely returns the target unchanged: 10^3 = 1000.
#[test]
fn execute_difference_with_missing_tool_still_succeeds() {
    let mut kernel = OcctKernel::new();
    let left = cube(&mut kernel, 10.0);
    let right_raw = cube(&mut kernel, 2.0);
    let right = translated_x(&mut kernel, right_raw, 100.0);

    let handle = kernel
        .execute(&GeometryOp::Difference { left, right })
        .expect("difference with a missing tool must succeed");
    assert_volume_near(&kernel, handle.id, 1000.0, "difference with missing tool");
}

/// A two-solid compound: BRepGProp sums the members, 2 * 10^3 = 2000.
#[test]
fn execute_union_of_disjoint_boxes_still_succeeds() {
    let mut kernel = OcctKernel::new();
    let left = cube(&mut kernel, 10.0);
    let right_raw = cube(&mut kernel, 10.0);
    let right = translated_x(&mut kernel, right_raw, 100.0);

    let handle = kernel
        .execute(&GeometryOp::Union { left, right })
        .expect("union of disjoint solids must succeed");
    assert_volume_near(&kernel, handle.id, 2000.0, "union of disjoint solids");
}
