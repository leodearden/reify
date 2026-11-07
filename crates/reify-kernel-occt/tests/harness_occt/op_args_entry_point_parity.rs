//! Every entry point to an op family decodes its arguments alike. Invalid
//! parameters draw the same text from `OcctKernel::execute`,
//! `OcctKernel::execute_with_history` and the family's typed `*_with_history`
//! primitive. Valid ones draw equal records from both of `OcctKernelHandle`'s
//! kernel-thread routes: its typed wrappers, which carry the bare `f64` they
//! were given, and `execute_with_history`, which decodes the op's `Value`
//! fields. The contract is equality across entry points, not any particular
//! wording or record content.
//!
//! Validation precedes any handle lookup on every path, so unissued handles
//! stand in for real geometry in the rejection cases.

#![cfg(has_occt)]

use reify_ir::{
    AttributeHistory, GeometryError, GeometryHandle, GeometryHandleId, GeometryOp, GeometryQuery,
    Value,
};
use reify_kernel_occt::{OcctKernel, OcctKernelHandle};

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

    for (d1, d2) in [(-MM, MM), (MM, f64::NAN)] {
        for edges in [vec![], vec![EDGE]] {
            let mode = if edges.is_empty() { "all" } else { "curated" };
            let typed_edges = edges.clone();
            cases.push((
                format!("asymmetric chamfer d1 {d1} d2 {d2}, {mode} edges"),
                GeometryOp::ChamferAsymmetric {
                    target: TARGET,
                    edges,
                    d1: Value::length(d1),
                    d2: Value::length(d2),
                },
                Box::new(move |k| {
                    k.chamfer_asymmetric_edges_with_history(TARGET, d1, d2, &typed_edges)
                        .map(|r| r.0)
                }),
            ));
        }
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

fn typed_result<R>(
    case: &str,
    result: Result<(GeometryHandleId, R), GeometryError>,
) -> (GeometryHandleId, R) {
    result.unwrap_or_else(|e| panic!("{case}: typed wrapper failed: {e:?}"))
}

/// Equal records alone would miss a mis-decoded length, which moves no
/// topology; the result's volume does not.
#[test]
fn handle_typed_wrappers_and_execute_with_history_build_the_same_result() {
    let kernel = OcctKernelHandle::spawn();
    let build = |op: GeometryOp| match kernel.execute(&op) {
        Ok(handle) => handle.id,
        Err(e) => panic!("fixture {op:?} failed: {e:?}"),
    };
    let volume = |case: &str, id: GeometryHandleId| match kernel
        .query(&GeometryQuery::Volume(id))
        .map(|v| v.as_f64())
    {
        Ok(Some(volume)) => volume,
        other => panic!("{case}: volume of {id:?} gave {other:?}"),
    };
    let square = || GeometryOp::RectangleProfile {
        width: Value::length(10.0 * MM),
        height: Value::length(10.0 * MM),
    };
    let profile = build(square());
    let path = build(GeometryOp::LineSegment {
        x1: 0.0,
        y1: 0.0,
        z1: 0.0,
        x2: 0.0,
        y2: 0.0,
        z2: 20.0 * MM,
    });
    let raised = build(GeometryOp::Translate {
        target: build(square()),
        dx: 0.0,
        dy: 0.0,
        dz: 10.0 * MM,
    });
    let distance = 5.0 * MM;
    let (axis_origin, axis_dir) = ([20.0 * MM, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let angle_rad = std::f64::consts::FRAC_PI_2;

    let (extruded, extrude_records) =
        typed_result("extrude", kernel.extrude_with_history(profile, distance));
    let (revolved, revolve_records) = typed_result(
        "revolve",
        kernel.revolve_with_history(profile, axis_origin, axis_dir, angle_rad),
    );
    let (swept, sweep_records) = typed_result("sweep", kernel.sweep_with_history(profile, path));
    let (lofted, loft_records) = typed_result("loft", kernel.loft_with_history(&[profile, raised]));
    let cases = [
        (
            "extrude",
            extruded,
            AttributeHistory::Extrude(extrude_records),
            GeometryOp::Extrude {
                profile,
                distance: Value::length(distance),
            },
        ),
        (
            "revolve",
            revolved,
            AttributeHistory::Revolve(revolve_records),
            GeometryOp::Revolve {
                profile,
                axis_origin,
                axis_dir,
                angle_rad,
            },
        ),
        (
            "sweep",
            swept,
            AttributeHistory::Sweep(sweep_records),
            GeometryOp::Sweep { profile, path },
        ),
        (
            "loft",
            lofted,
            AttributeHistory::Loft(loft_records),
            GeometryOp::Loft {
                profiles: vec![profile, raised],
            },
        ),
    ];
    for (case, typed_id, typed_history, op) in cases {
        let (handle, history) = kernel
            .execute_with_history(&op)
            .unwrap_or_else(|e| panic!("{case}: execute_with_history failed: {e:?}"));
        assert_eq!(history, typed_history, "{case}: records");
        assert_eq!(
            volume(case, handle.id),
            volume(case, typed_id),
            "{case}: volume"
        );
    }
}
