//! The C4 kernel LENGTH tripwire through the real OCCT path: bare LENGTH inputs are
//! reported (and, when armed, panic in debug), dimensioned ones are not, and the
//! deliberately-ungated fields stay ungated.

#![cfg(has_occt)]

use crate::support::{assert_ungated_fields_are_bare, make_box_20_10_5};
use reify_ir::{GeometryHandleId, GeometryOp, Value};
use reify_kernel_occt::OcctKernel;

// ── C4 kernel LENGTH tripwire (task #5751) ───────────────────────────────
//
// Boundary rows 13/14 of `docs/prds/v0_6/units-length-gate-completion.md`
// through the real OCCT path. The tripwire is a DETECTOR, never a gate.
//
// Every kernel here is built with `OcctKernel::new()` on the TEST THREAD
// (as `make_box_20_10_5` already does) rather than through
// `OcctKernelHandle`, whose worker thread would not see the thread-local
// arm.

/// End-to-end wiring proof through `OcctKernel::execute`: one diagnostic
/// per bare Box dimension, naming the op kind and that field.
#[test]
fn occt_execute_box_bare_dimensions_emits_tripwire_diagnostics() {
    reify_test_support::prime_tracing_callsite_cache();

    let mut kernel = OcctKernel::new();
    let (subscriber, capture) = reify_test_support::warn_capturing_subscriber();
    let result = tracing::subscriber::with_default(subscriber, || {
        kernel.execute(&GeometryOp::Box {
            size_x: Value::Real(20.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(5.0),
        })
    });

    capture.assert_count_and_any_message_contains(3, "Box");
    capture.assert_any_event_field_contains("field", "size_x");
    capture.assert_any_event_field_contains("field", "size_y");
    capture.assert_any_event_field_contains("field", "size_z");
    assert!(
        result.is_ok(),
        "the tripwire is a detector, not a gate: {result:?}"
    );
}

/// Negative control: dimensioned LENGTH inputs emit ZERO diagnostics.
#[test]
fn occt_execute_dimensioned_box_emits_no_tripwire_diagnostic() {
    reify_test_support::prime_tracing_callsite_cache();

    let mut kernel = OcctKernel::new();
    let (subscriber, capture) = reify_test_support::warn_capturing_subscriber();
    let result = tracing::subscriber::with_default(subscriber, || {
        kernel.execute(&GeometryOp::Box {
            size_x: Value::length(0.020),
            size_y: Value::length(0.010),
            size_z: Value::length(0.005),
        })
    });

    capture.assert_count(0);
    assert!(result.is_ok(), "{result:?}");
}

/// Boundary row 13 through the real OCCT path, half 1 (op kind).
///
/// `#[cfg(debug_assertions)]` is mandatory on a `#[should_panic]` test for
/// a debug-only assertion — see the rule documented at
/// `crates/reify-kernel-occt/src/lib.rs` `mod tests` (the three
/// `"must be a parent_handle key"` tests).
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "Box")]
fn occt_armed_bare_length_panics_naming_the_op_kind() {
    let _g = reify_ir::arm_length_tripwire_assert();
    let mut kernel = OcctKernel::new();
    let _ = kernel.execute(&GeometryOp::Box {
        size_x: Value::Real(20.0),
        size_y: Value::Real(10.0),
        size_z: Value::Real(5.0),
    });
}

/// Boundary row 13 through the real OCCT path, half 2 (field name).
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "size_x")]
fn occt_armed_bare_length_panics_naming_the_field() {
    let _g = reify_ir::arm_length_tripwire_assert();
    let mut kernel = OcctKernel::new();
    let _ = kernel.execute(&GeometryOp::Box {
        size_x: Value::Real(20.0),
        size_y: Value::Real(10.0),
        size_z: Value::Real(5.0),
    });
}

/// Regression teeth: a migration that retypes `CircularPattern.angle` to
/// a dimensioned `Value` (task 5781's exact shape) must red here instead
/// of leaving `occt_non_length_fields_stay_ungated` green with its
/// premise silently gone.
#[test]
#[should_panic(expected = "CircularPattern.angle")]
fn ungated_fixture_guard_reds_on_a_dimensioned_circular_pattern_angle() {
    assert_ungated_fields_are_bare(&GeometryOp::CircularPattern {
        target: GeometryHandleId(1),
        axis_origin: [0.0; 3],
        axis_dir: [0.0, 0.0, 1.0],
        count: 2,
        angle: Value::angle(std::f64::consts::PI),
    });
}

/// Regression teeth: a migration that retypes `Draft.angle` to a
/// dimensioned `Value` (task 5780's exact shape) must red here instead
/// of leaving `occt_non_length_fields_stay_ungated` green with its
/// premise silently gone.
#[test]
#[should_panic(expected = "Draft.angle")]
fn ungated_fixture_guard_reds_on_a_dimensioned_draft_angle() {
    assert_ungated_fields_are_bare(&GeometryOp::Draft {
        target: GeometryHandleId(1),
        faces: vec![],
        angle: Value::angle(0.05),
        plane: GeometryHandleId(1),
    });
}

/// Pins that the guard keys on the `Scalar` VARIANT, not on the
/// dimension being non-dimensionless: a DIMENSIONLESS `Scalar` must
/// still red, because a bare `Value::Real` is the one shape
/// `check_length_field` can never wave through.
#[test]
#[should_panic(expected = "HalfSpace.ny")]
fn ungated_fixture_guard_reds_on_a_dimensioned_half_space_normal() {
    assert_ungated_fields_are_bare(&GeometryOp::HalfSpace {
        px: Value::length(0.0),
        py: Value::length(0.0),
        pz: Value::length(0.0),
        nx: Value::Real(0.0),
        ny: Value::Scalar {
            si_value: 0.0,
            dimension: reify_core::DimensionVector::DIMENSIONLESS,
        },
        nz: Value::Real(1.0),
    });
}

/// Closes the guard's own vacuity hole: an op kind it does not
/// recognise must fail loudly rather than silently assert nothing.
#[test]
#[should_panic(expected = "no deliberately-ungated field")]
fn ungated_fixture_guard_reds_on_an_op_with_no_ungated_field() {
    assert_ungated_fields_are_bare(&GeometryOp::Sphere {
        radius: Value::Real(1.0),
    });
}

/// The anti-over-reach control: the FIVE deliberately ungated OCCT fields.
///
/// The PRD's split is 47 = 42 + 3 + 2. The 3 are `HalfSpace`'s `nx`/`ny`/`nz`
/// — dimensionless unit-normal components, not lengths. The 2 are
/// `CircularPattern.angle` and `Draft.angle` — ANGLE, which is PRD 3's
/// surface, not this one. All five must stay on the plain, context-free
/// `extract_f64`.
///
/// Without this control a blanket conversion of all 46 sites would pass
/// every other test in this module.
///
/// Bareness of the five fixtures below is now ENFORCED by
/// `assert_ungated_fields_are_bare`, not merely asserted in a comment:
/// the control cannot pass vacuously if a fixture is later dimensioned.
#[test]
fn occt_non_length_fields_stay_ungated() {
    reify_test_support::prime_tracing_callsite_cache();

    /// Assert no captured WARN names `field` in its message or its
    /// structured `field` value.
    fn assert_no_warn_for_field(capture: &reify_test_support::WarnCapture, field: &str) {
        for fields in capture.fields_by_event() {
            assert_ne!(
                fields.get("field").map(String::as_str),
                Some(field),
                "{field} must stay on the ungated extract_f64; captured: {:?}",
                capture.messages()
            );
        }
    }

    // 3 dimensionless unit-normal components. Point coords are properly
    // dimensioned, so a correct build emits nothing at all here.
    let mut kernel = OcctKernel::new();
    let half_space = GeometryOp::HalfSpace {
        px: Value::length(0.0),
        py: Value::length(0.0),
        pz: Value::length(0.0),
        nx: Value::Real(0.0),
        ny: Value::Real(0.0),
        nz: Value::Real(1.0),
    };
    assert_ungated_fields_are_bare(&half_space);
    let (subscriber, capture) = reify_test_support::warn_capturing_subscriber();
    tracing::subscriber::with_default(subscriber, || {
        let _ = kernel.execute(&half_space);
    });
    for n in ["nx", "ny", "nz"] {
        assert_no_warn_for_field(&capture, n);
    }
    capture.assert_count(0);

    // 2 ANGLE fields. A bare `Value::Real` in each: if either had been
    // converted, a `field = "angle"` warn would appear.
    let mut kernel = OcctKernel::new();
    let target = make_box_20_10_5(&mut kernel);
    let circular_pattern = GeometryOp::CircularPattern {
        target,
        axis_origin: [0.0, 0.0, 0.0],
        axis_dir: [0.0, 0.0, 1.0],
        count: 2,
        // Stays bare deliberately — task 5777. ε (5781) migrates
        // `circular_pattern` angles, but NOT this one: it is a control
        // for the 46 = 41 + 3 + 2 ungated-field split, not a corpus
        // fixture, and bareness is enforced below by
        // `assert_ungated_fields_are_bare`. See
        // docs/notes/angle-literal-migration-ledger.md §1.2.1.
        angle: Value::Real(std::f64::consts::PI),
    };
    let draft = GeometryOp::Draft {
        target,
        faces: vec![],
        // Stays bare deliberately — task 5777, same control contract as
        // the arm above, but δ's (5780): `draft` is δ's migration
        // target.
        angle: Value::Real(0.05),
        plane: target,
    };
    assert_ungated_fields_are_bare(&circular_pattern);
    assert_ungated_fields_are_bare(&draft);
    let (subscriber, capture) = reify_test_support::warn_capturing_subscriber();
    tracing::subscriber::with_default(subscriber, || {
        let _ = kernel.execute(&circular_pattern);
        let _ = kernel.execute(&draft);
    });
    assert_no_warn_for_field(&capture, "angle");
}

/// **Full 41-field enumeration (step-9B).**
///
/// The completeness check for the PRD's 47 = 42 + 3 + 2 split, proved by
/// OBSERVATION rather than by asserting a table against itself: every one of
/// the 41 LENGTH-semantic `(op, field)` pairs is driven through
/// `OcctKernel::execute` with a bare `Value::Real` in the field under test
/// and a properly dimensioned value in every other length field of the same
/// op, so exactly ONE diagnostic is expected and it must name that exact op
/// kind and field.
///
/// Assertions are on the emitted WARN only, never on the `Result`: the
/// tripwire fires before the OCCT call, so a downstream geometric failure
/// cannot invalidate the assertion — and asserting on `Result` would make
/// this test hostage to OCCT behaviour.
///
/// A site left on the plain context-free `extract_f64` fails here.
#[test]
fn occt_every_length_field_is_gated() {
    reify_test_support::prime_tracing_callsite_cache();

    let mut kernel = OcctKernel::new();
    // Ops taking a target/profile/path handle get one real Box. Five arms
    // (LinearPattern, Thicken, ZoneSlab, OffsetSolid, LinearPattern2D)
    // resolve the handle BEFORE extraction, so it must be valid.
    let target = make_box_20_10_5(&mut kernel);

    let cases: Vec<(GeometryOp, &str, &str)> = vec![
        (
            GeometryOp::Box {
                    size_x: Value::Real(2.0),
                    size_y: Value::length(3.0),
                    size_z: Value::length(4.0),
            },
            "Box",
            "size_x",
        ),
        (
            GeometryOp::Box {
                    size_x: Value::length(2.0),
                    size_y: Value::Real(3.0),
                    size_z: Value::length(4.0),
            },
            "Box",
            "size_y",
        ),
        (
            GeometryOp::Box {
                    size_x: Value::length(2.0),
                    size_y: Value::length(3.0),
                    size_z: Value::Real(4.0),
            },
            "Box",
            "size_z",
        ),
        (
            GeometryOp::Cylinder {
                    radius: Value::Real(1.0),
                    height: Value::length(2.0),
            },
            "Cylinder",
            "radius",
        ),
        (
            GeometryOp::Cylinder {
                    radius: Value::length(1.0),
                    height: Value::Real(2.0),
            },
            "Cylinder",
            "height",
        ),
        (
            GeometryOp::Sphere {
                    radius: Value::Real(1.0),
            },
            "Sphere",
            "radius",
        ),
        (
            GeometryOp::Tube {
                    outer_r: Value::Real(2.0),
                    inner_r: Value::length(1.0),
                    height: Value::length(3.0),
            },
            "Tube",
            "outer_r",
        ),
        (
            GeometryOp::Tube {
                    outer_r: Value::length(2.0),
                    inner_r: Value::Real(1.0),
                    height: Value::length(3.0),
            },
            "Tube",
            "inner_r",
        ),
        (
            GeometryOp::Tube {
                    outer_r: Value::length(2.0),
                    inner_r: Value::length(1.0),
                    height: Value::Real(3.0),
            },
            "Tube",
            "height",
        ),
        (
            GeometryOp::Cone {
                    bottom_radius: Value::Real(2.0),
                    top_radius: Value::length(1.0),
                    height: Value::length(3.0),
            },
            "Cone",
            "bottom_radius",
        ),
        (
            GeometryOp::Cone {
                    bottom_radius: Value::length(2.0),
                    top_radius: Value::Real(1.0),
                    height: Value::length(3.0),
            },
            "Cone",
            "top_radius",
        ),
        (
            GeometryOp::Cone {
                    bottom_radius: Value::length(2.0),
                    top_radius: Value::length(1.0),
                    height: Value::Real(3.0),
            },
            "Cone",
            "height",
        ),
        (
            GeometryOp::Wedge {
                    width: Value::Real(2.0),
                    depth: Value::length(3.0),
                    height: Value::length(4.0),
                    top_width: Value::length(1.0),
            },
            "Wedge",
            "width",
        ),
        (
            GeometryOp::Wedge {
                    width: Value::length(2.0),
                    depth: Value::Real(3.0),
                    height: Value::length(4.0),
                    top_width: Value::length(1.0),
            },
            "Wedge",
            "depth",
        ),
        (
            GeometryOp::Wedge {
                    width: Value::length(2.0),
                    depth: Value::length(3.0),
                    height: Value::Real(4.0),
                    top_width: Value::length(1.0),
            },
            "Wedge",
            "height",
        ),
        (
            GeometryOp::Wedge {
                    width: Value::length(2.0),
                    depth: Value::length(3.0),
                    height: Value::length(4.0),
                    top_width: Value::Real(1.0),
            },
            "Wedge",
            "top_width",
        ),
        (
            GeometryOp::Torus {
                    major_radius: Value::Real(3.0),
                    minor_radius: Value::length(1.0),
            },
            "Torus",
            "major_radius",
        ),
        (
            GeometryOp::Torus {
                    major_radius: Value::length(3.0),
                    minor_radius: Value::Real(1.0),
            },
            "Torus",
            "minor_radius",
        ),
        (
            GeometryOp::HalfSpace {
                    nx: Value::Real(0.0),
                    ny: Value::Real(0.0),
                    nz: Value::Real(1.0),
                    px: Value::Real(0.0),
                    py: Value::length(0.0),
                    pz: Value::length(0.0),
            },
            "HalfSpace",
            "px",
        ),
        (
            GeometryOp::HalfSpace {
                    nx: Value::Real(0.0),
                    ny: Value::Real(0.0),
                    nz: Value::Real(1.0),
                    px: Value::length(0.0),
                    py: Value::Real(0.0),
                    pz: Value::length(0.0),
            },
            "HalfSpace",
            "py",
        ),
        (
            GeometryOp::HalfSpace {
                    nx: Value::Real(0.0),
                    ny: Value::Real(0.0),
                    nz: Value::Real(1.0),
                    px: Value::length(0.0),
                    py: Value::length(0.0),
                    pz: Value::Real(0.0),
            },
            "HalfSpace",
            "pz",
        ),
        (
            GeometryOp::Fillet {
                    target,
                    edges: vec![],
                    radius: Value::Real(0.1),
            },
            "Fillet",
            "radius",
        ),
        (
            GeometryOp::Chamfer {
                    target,
                    edges: vec![],
                    distance: Value::Real(0.1),
            },
            "Chamfer",
            "distance",
        ),
        (
            GeometryOp::ChamferAsymmetric {
                    target,
                    edges: vec![],
                    d1: Value::Real(0.1),
                    d2: Value::length(0.2),
            },
            "ChamferAsymmetric",
            "d1",
        ),
        (
            GeometryOp::ChamferAsymmetric {
                    target,
                    edges: vec![],
                    d1: Value::length(0.1),
                    d2: Value::Real(0.2),
            },
            "ChamferAsymmetric",
            "d2",
        ),
        (
            GeometryOp::LinearPattern {
                    target,
                    direction: [1.0, 0.0, 0.0],
                    count: 2,
                    spacing: Value::Real(1.0),
            },
            "LinearPattern",
            "spacing",
        ),
        (
            GeometryOp::Thicken {
                    target,
                    offset: Value::Real(0.1),
            },
            "Thicken",
            "offset",
        ),
        (
            GeometryOp::OffsetCurve {
                    target,
                    reference: None,
                    direction: None,
                    distance: Value::Real(0.1),
            },
            "OffsetCurve",
            "distance",
        ),
        (
            GeometryOp::ZoneSlab {
                    target,
                    width: Value::Real(1.0),
            },
            "ZoneSlab",
            "width",
        ),
        (
            GeometryOp::OffsetSolid {
                    target,
                    distance: Value::Real(0.1),
            },
            "OffsetSolid",
            "distance",
        ),
        (
            GeometryOp::Shell {
                    target,
                    faces_to_remove: vec![],
                    open_face_handles: vec![],
                    thickness: Value::Real(0.1),
            },
            "Shell",
            "thickness",
        ),
        (
            GeometryOp::Extrude {
                    profile: target,
                    distance: Value::Real(1.0),
            },
            "Extrude",
            "distance",
        ),
        (
            GeometryOp::Pipe {
                    path: target,
                    radius: Value::Real(0.5),
            },
            "Pipe",
            "radius",
        ),
        (
            GeometryOp::ExtrudeSymmetric {
                    profile: target,
                    distance: Value::Real(1.0),
            },
            "ExtrudeSymmetric",
            "distance",
        ),
        (
            GeometryOp::LinearPattern2D {
                    target,
                    direction1: [1.0, 0.0, 0.0],
                    count1: 2,
                    direction2: [0.0, 1.0, 0.0],
                    count2: 2,
                    spacing1: Value::Real(1.0),
                    spacing2: Value::length(2.0),
            },
            "LinearPattern2D",
            "spacing1",
        ),
        (
            GeometryOp::LinearPattern2D {
                    target,
                    direction1: [1.0, 0.0, 0.0],
                    count1: 2,
                    direction2: [0.0, 1.0, 0.0],
                    count2: 2,
                    spacing1: Value::length(1.0),
                    spacing2: Value::Real(2.0),
            },
            "LinearPattern2D",
            "spacing2",
        ),
        (
            GeometryOp::RectangleProfile {
                    width: Value::Real(2.0),
                    height: Value::length(3.0),
            },
            "RectangleProfile",
            "width",
        ),
        (
            GeometryOp::RectangleProfile {
                    width: Value::length(2.0),
                    height: Value::Real(3.0),
            },
            "RectangleProfile",
            "height",
        ),
        (
            GeometryOp::CircleProfile {
                    radius: Value::Real(1.0),
            },
            "CircleProfile",
            "radius",
        ),
        (
            GeometryOp::EllipseProfile {
                    semi_major: Value::Real(2.0),
                    semi_minor: Value::length(1.0),
            },
            "EllipseProfile",
            "semi_major",
        ),
        (
            GeometryOp::EllipseProfile {
                    semi_major: Value::length(2.0),
                    semi_minor: Value::Real(1.0),
            },
            "EllipseProfile",
            "semi_minor",
        ),
    ];
    assert_eq!(
        cases.len(),
        41,
        "the PRD's 47 = 42 + 3 + 2 split: 41 LENGTH-semantic (op, field) pairs"
    );

    for (op, op_kind, field) in cases {
        let (subscriber, capture) = reify_test_support::warn_capturing_subscriber();
        tracing::subscriber::with_default(subscriber, || {
            let _ = kernel.execute(&op);
        });
        capture.assert_count_and_any_message_contains(1, op_kind);
        capture.assert_any_event_field_contains("op_kind", op_kind);
        capture.assert_any_event_field_contains("field", field);
        // Step-9D's per-kernel half: the kernel emits the SHARED formatter's
        // string verbatim, never a re-rolled literal.
        assert_eq!(
            capture.messages()[0],
            reify_ir::check_length_field(op_kind, field, &Value::Real(1.0))
                .expect("a bare Real is a violation"),
            "{op_kind}.{field}: message drifted from the shared formatter"
        );
    }
}

/// **Release contract, boundary row 14 (step-9C).**
///
/// In a release build the assertion is compiled out entirely, so even with
/// the arm held a bare-`Value::Real` `execute` must (i) not panic,
/// (ii) still emit the diagnostic naming op kind and field, and (iii) return
/// the SAME disposition as the dimensioned control.
///
/// This is the half a debug-only test cannot reach. It executes for real on
/// the merge gate, which forces `--profile both` (`scripts/verify.sh`,
/// `DF_VERIFY_ROLE=merge`).
#[cfg(not(debug_assertions))]
#[test]
fn occt_release_armed_bare_length_reports_without_panicking() {
    reify_test_support::prime_tracing_callsite_cache();

    let _g = reify_ir::arm_length_tripwire_assert();
    assert!(reify_ir::length_tripwire_assert_armed());

    let mut kernel = OcctKernel::new();
    let (subscriber, capture) = reify_test_support::warn_capturing_subscriber();
    let bare = tracing::subscriber::with_default(subscriber, || {
        kernel.execute(&GeometryOp::Box {
            size_x: Value::Real(0.020),
            size_y: Value::Real(0.010),
            size_z: Value::Real(0.005),
        })
    });

    capture.assert_count_and_any_message_contains(3, "Box");
    capture.assert_any_event_field_contains("field", "size_x");

    // Same SI magnitudes, properly dimensioned: the disposition must match.
    let control = kernel.execute(&GeometryOp::Box {
        size_x: Value::length(0.020),
        size_y: Value::length(0.010),
        size_z: Value::length(0.005),
    });
    assert_eq!(
        bare.is_ok(),
        control.is_ok(),
        "armed release build changed the accept/reject disposition: \
         bare={bare:?} control={control:?}"
    );
}
