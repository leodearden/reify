//! Every entry point to an op family rejects the same invalid parameters with
//! the same text: `OcctKernel::execute`, `OcctKernel::execute_with_history`
//! and the family's typed `*_with_history` primitive. The contract is equality
//! across entry points, not any particular wording.
//!
//! Validation precedes any handle lookup on every path, so unissued handles
//! stand in for real geometry.

#![cfg(has_occt)]

use reify_ir::{GeometryError, GeometryHandle, GeometryHandleId, GeometryOp, Value};
use reify_kernel_occt::OcctKernel;

const PROFILE: GeometryHandleId = GeometryHandleId(3);
const TARGET: GeometryHandleId = GeometryHandleId(5);
const EDGE: GeometryHandleId = GeometryHandleId(7);
const MM: f64 = 1e-3;

type Primitive = Box<dyn Fn(&mut OcctKernel) -> Result<GeometryHandle, GeometryError>>;

fn message(case: &str, entry: &str, result: Result<GeometryHandle, GeometryError>) -> String {
    match result {
        Err(GeometryError::OperationFailed(m)) => m,
        Err(other) => panic!("{case}: {entry} failed with {other:?}, not OperationFailed"),
        Ok(_) => panic!("{case}: {entry} accepted invalid parameters"),
    }
}

/// `(case, op, the typed primitive for the same parameters)`.
fn cases() -> Vec<(String, GeometryOp, Primitive)> {
    let mut cases: Vec<(String, GeometryOp, Primitive)> = Vec::new();

    for distance in [f64::NAN, f64::INFINITY, 0.0] {
        cases.push((
            format!("extrude distance {distance}"),
            GeometryOp::Extrude {
                profile: PROFILE,
                distance: Value::length(distance),
            },
            Box::new(move |k| k.extrude_with_history(PROFILE, distance).map(|r| r.0)),
        ));
    }

    let unit_y = [0.0, 1.0, 0.0];
    let quarter = std::f64::consts::FRAC_PI_2;
    for (label, origin, dir, angle) in [
        ("non-finite origin", [f64::NAN, 0.0, 0.0], unit_y, quarter),
        (
            "non-finite dir",
            [0.0; 3],
            [0.0, f64::INFINITY, 0.0],
            quarter,
        ),
        ("NaN angle", [0.0; 3], unit_y, f64::NAN),
        ("zero angle", [0.0; 3], unit_y, 0.0),
        ("zero-length dir", [0.0; 3], [0.0; 3], quarter),
    ] {
        cases.push((
            format!("revolve {label}"),
            GeometryOp::Revolve {
                profile: PROFILE,
                axis_origin: origin,
                axis_dir: dir,
                angle_rad: angle,
            },
            Box::new(move |k| {
                k.revolve_with_history(PROFILE, origin, dir, angle)
                    .map(|r| r.0)
            }),
        ));
    }

    for profiles in [vec![PROFILE], vec![]] {
        let typed = profiles.clone();
        cases.push((
            format!("loft of {} profile(s)", profiles.len()),
            GeometryOp::Loft { profiles },
            Box::new(move |k| k.loft_with_history(&typed).map(|r| r.0)),
        ));
    }

    for size in [-MM, f64::NAN] {
        cases.push((
            format!("fillet radius {size}, all edges"),
            GeometryOp::Fillet {
                target: TARGET,
                edges: vec![],
                radius: Value::length(size),
            },
            Box::new(move |k| k.fillet_with_history(TARGET, size).map(|r| r.0)),
        ));
        cases.push((
            format!("fillet radius {size}, curated edges"),
            GeometryOp::Fillet {
                target: TARGET,
                edges: vec![EDGE],
                radius: Value::length(size),
            },
            Box::new(move |k| {
                k.fillet_edges_with_history(TARGET, size, &[EDGE])
                    .map(|r| r.0)
            }),
        ));
        cases.push((
            format!("chamfer distance {size}, all edges"),
            GeometryOp::Chamfer {
                target: TARGET,
                edges: vec![],
                distance: Value::length(size),
            },
            Box::new(move |k| k.chamfer_with_history(TARGET, size).map(|r| r.0)),
        ));
        cases.push((
            format!("chamfer distance {size}, curated edges"),
            GeometryOp::Chamfer {
                target: TARGET,
                edges: vec![EDGE],
                distance: Value::length(size),
            },
            Box::new(move |k| {
                k.chamfer_edges_with_history(TARGET, size, &[EDGE])
                    .map(|r| r.0)
            }),
        ));
    }

    cases
}

#[test]
fn invalid_parameters_yield_identical_errors_on_every_entry_point() {
    let mut kernel = OcctKernel::new();
    for (case, op, primitive) in cases() {
        let via_execute = message(&case, "execute", kernel.execute(&op));
        let via_history = message(
            &case,
            "execute_with_history",
            kernel.execute_with_history(&op).map(|r| r.0),
        );
        let via_primitive = message(&case, "typed primitive", primitive(&mut kernel));
        assert_eq!(
            via_history, via_execute,
            "{case}: execute_with_history vs execute"
        );
        assert_eq!(
            via_primitive, via_execute,
            "{case}: typed primitive vs execute"
        );
    }
}
