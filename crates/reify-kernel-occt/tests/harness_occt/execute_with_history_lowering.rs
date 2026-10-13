//! `OcctKernel::execute_with_history` is the one op-to-history lowering: each
//! history op reaches its record kind, and its length fields decode exactly as
//! `execute` decodes them.
//!
//! Driven in-process on the test thread, because the length-tripwire arm is
//! thread-local and does not cross `OcctKernelHandle`'s worker thread.

#![cfg(has_occt)]

use reify_ir::{AttributeHistory, GeometryError, GeometryHandleId, GeometryOp, Value};
use reify_kernel_occt::OcctKernel;

const MM: f64 = 1e-3;

/// A handle the kernel never issued: length decode runs before any lookup.
const UNISSUED: GeometryHandleId = GeometryHandleId(1);

fn make(kernel: &mut OcctKernel, op: GeometryOp) -> GeometryHandleId {
    match kernel.execute(&op) {
        Ok(handle) => handle.id,
        Err(e) => panic!("fixture {op:?} failed: {e:?}"),
    }
}

fn cube_10mm(kernel: &mut OcctKernel) -> GeometryHandleId {
    make(
        kernel,
        GeometryOp::Box {
            width: Value::length(10.0 * MM),
            height: Value::length(10.0 * MM),
            depth: Value::length(10.0 * MM),
        },
    )
}

fn square_10mm(kernel: &mut OcctKernel) -> GeometryHandleId {
    make(
        kernel,
        GeometryOp::RectangleProfile {
            width: Value::length(10.0 * MM),
            height: Value::length(10.0 * MM),
        },
    )
}

fn record_kind(history: &AttributeHistory) -> &'static str {
    match history {
        AttributeHistory::None => "None",
        AttributeHistory::Extrude(_) => "Extrude",
        AttributeHistory::Revolve(_) => "Revolve",
        AttributeHistory::Sweep(_) => "Sweep",
        AttributeHistory::Loft(_) => "Loft",
        AttributeHistory::Boolean(_) => "Boolean",
        AttributeHistory::LocalFeature(_) => "LocalFeature",
    }
}

#[test]
fn history_lowering_routes_each_op_to_its_record_kind() {
    let mut kernel = OcctKernel::new();
    let a = cube_10mm(&mut kernel);
    let unmoved_b = cube_10mm(&mut kernel);
    let b = make(
        &mut kernel,
        GeometryOp::Translate {
            target: unmoved_b,
            dx: 5.0 * MM,
            dy: 0.0,
            dz: 0.0,
        },
    );
    let profile = square_10mm(&mut kernel);
    let path = make(
        &mut kernel,
        GeometryOp::LineSegment {
            x1: 0.0,
            y1: 0.0,
            z1: 0.0,
            x2: 0.0,
            y2: 0.0,
            z2: 20.0 * MM,
        },
    );
    let unraised = square_10mm(&mut kernel);
    let raised = make(
        &mut kernel,
        GeometryOp::Translate {
            target: unraised,
            dx: 0.0,
            dy: 0.0,
            dz: 10.0 * MM,
        },
    );
    let edge = kernel.extract_edges(a).expect("box edges")[0];

    let cases = [
        (
            GeometryOp::Extrude {
                profile,
                distance: Value::length(5.0 * MM),
            },
            "Extrude",
        ),
        (
            GeometryOp::Revolve {
                profile,
                axis_origin: [20.0 * MM, 0.0, 0.0],
                axis_dir: [0.0, 1.0, 0.0],
                angle_rad: std::f64::consts::FRAC_PI_2,
            },
            "Revolve",
        ),
        (GeometryOp::Sweep { profile, path }, "Sweep"),
        (
            GeometryOp::Loft {
                profiles: vec![profile, raised],
            },
            "Loft",
        ),
        (GeometryOp::Union { left: a, right: b }, "Boolean"),
        (GeometryOp::Difference { left: a, right: b }, "Boolean"),
        (GeometryOp::Intersection { left: a, right: b }, "Boolean"),
        (
            GeometryOp::Fillet {
                target: a,
                edges: vec![],
                radius: Value::length(MM),
            },
            "LocalFeature",
        ),
        (
            GeometryOp::Fillet {
                target: a,
                edges: vec![edge],
                radius: Value::length(MM),
            },
            "LocalFeature",
        ),
        (
            GeometryOp::Chamfer {
                target: a,
                edges: vec![],
                distance: Value::length(MM),
            },
            "LocalFeature",
        ),
        (
            GeometryOp::Chamfer {
                target: a,
                edges: vec![edge],
                distance: Value::length(MM),
            },
            "LocalFeature",
        ),
        (
            GeometryOp::Box {
                width: Value::length(10.0 * MM),
                height: Value::length(10.0 * MM),
                depth: Value::length(10.0 * MM),
            },
            "None",
        ),
    ];
    for (op, expected) in cases {
        match kernel.execute_with_history(&op) {
            Ok((_, history)) => assert_eq!(record_kind(&history), expected, "{op:?}"),
            Err(e) => panic!("{op:?} failed: {e:?}"),
        }
    }
}

/// Each history op's length field, holding `value`, with an unissued target.
fn length_field_cases(value: Value) -> [(GeometryOp, &'static str); 3] {
    [
        (
            GeometryOp::Extrude {
                profile: UNISSUED,
                distance: value.clone(),
            },
            "distance",
        ),
        (
            GeometryOp::Fillet {
                target: UNISSUED,
                edges: vec![],
                radius: value.clone(),
            },
            "radius",
        ),
        (
            GeometryOp::Chamfer {
                target: UNISSUED,
                edges: vec![],
                distance: value,
            },
            "distance",
        ),
    ]
}

#[cfg(debug_assertions)]
#[test]
fn armed_tripwire_panics_on_history_path_with_canonical_message() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    for (op, field) in length_field_cases(Value::Real(1.0)) {
        let mut kernel = OcctKernel::new();
        let _g = reify_ir::arm_length_tripwire_assert();
        let outcome = catch_unwind(AssertUnwindSafe(|| kernel.execute_with_history(&op)));
        let Err(payload) = outcome else {
            panic!("armed bare Real at {op:?} did not panic");
        };
        let Ok(message) = payload.downcast::<String>() else {
            panic!("{op:?} panicked with a non-String payload");
        };
        assert_eq!(
            *message,
            reify_ir::non_length_kernel_field_message(op.kind_name(), field, "Real")
        );
    }
}

fn operation_failed_message(result: Result<impl Sized, GeometryError>, op: &GeometryOp) -> String {
    match result {
        Err(GeometryError::OperationFailed(message)) => message,
        Err(other) => panic!("{op:?}: expected OperationFailed, got {other:?}"),
        Ok(_) => panic!("{op:?}: a non-numeric length field was accepted"),
    }
}

#[test]
fn history_path_non_numeric_length_error_equals_execute() {
    let mut kernel = OcctKernel::new();
    for (op, field) in length_field_cases(Value::String("x".into())) {
        let expected = reify_ir::non_numeric_kernel_field_message(op.kind_name(), field);
        let via_history = operation_failed_message(kernel.execute_with_history(&op), &op);
        let via_execute = operation_failed_message(kernel.execute(&op), &op);
        assert_eq!(via_history, expected, "execute_with_history");
        assert_eq!(via_execute, expected, "execute");
    }
}
