//! Serial and parallel OCCT boolean builds produce bit-identical results (task 7439).
//!
//! Persistent naming consumes a boolean result's sub-shape ORDER and its
//! Modified/Generated/Deleted history, so each test runs one operation once
//! Serial and repeatedly Parallel, each on a fresh kernel, and requires the
//! topology, sub-shape order, per-sub-shape geometry and history to match
//! exactly: every float is compared by its bits, never within a tolerance.

#![cfg(all(has_occt, feature = "test-fixtures"))]

use std::fmt::Debug;

use reify_ir::{
    BRepKind, BooleanOpHistoryRecords, GeometryHandleId, GeometryOp, GeometryQuery, Value,
};
use reify_kernel_occt::{
    BooleanParallelism, OcctKernel, boolean_parallelism, with_boolean_parallelism,
};

const PLATE_SIDE: f64 = 0.2;
const PLATE_THICKNESS: f64 = 0.005;
const HOLE_RADIUS: f64 = 0.003;
/// Grid pitch that keeps neighbouring holes apart (pitch > hole diameter).
const DISJOINT_PITCH: f64 = 0.015;
/// Grid pitch that makes neighbouring holes overlap (pitch < hole diameter).
const OVERLAPPING_PITCH: f64 = 0.004;

/// Bit-exact identity of a shape as persistent naming sees it: sub-shapes in
/// canonical `TopExp` order, each pinned by its geometry.
#[derive(Debug, PartialEq)]
struct ShapeFingerprint {
    repr: Option<BRepKind>,
    volume_bits: u64,
    /// Per face, in `extract_faces` order: (area bits, centroid x/y/z bits).
    faces: Vec<(u64, [u64; 3])>,
    /// Per edge, in `extract_edges` order: bounding-box min/max bits.
    edges: Vec<[u64; 6]>,
    vertex_count: usize,
}

fn real_bits(kernel: &OcctKernel, query: &GeometryQuery) -> u64 {
    kernel
        .query(query)
        .unwrap_or_else(|e| panic!("{query:?} must succeed: {e}"))
        .as_f64()
        .unwrap_or_else(|| panic!("{query:?} must answer a real"))
        .to_bits()
}

/// Bits of the named reals in a query that answers a JSON object.
fn json_real_bits<const N: usize>(
    kernel: &OcctKernel,
    query: &GeometryQuery,
    keys: [&str; N],
) -> [u64; N] {
    let Value::String(json) = kernel
        .query(query)
        .unwrap_or_else(|e| panic!("{query:?} must succeed: {e}"))
    else {
        panic!("{query:?} must answer a JSON string");
    };
    let object: serde_json::Value =
        serde_json::from_str(&json).unwrap_or_else(|e| panic!("{json:?} must be JSON: {e}"));
    keys.map(|key| {
        object[key]
            .as_f64()
            .unwrap_or_else(|| panic!("{json:?} must carry a numeric {key:?}"))
            .to_bits()
    })
}

fn fingerprint(kernel: &mut OcctKernel, shape: GeometryHandleId) -> ShapeFingerprint {
    let faces = kernel
        .extract_faces(shape)
        .expect("extract_faces must succeed");
    let edges = kernel
        .extract_edges(shape)
        .expect("extract_edges must succeed");
    let vertex_count = kernel
        .extract_vertices(shape)
        .expect("extract_vertices must succeed")
        .len();
    ShapeFingerprint {
        repr: kernel.repr_of(shape),
        volume_bits: real_bits(kernel, &GeometryQuery::Volume(shape)),
        faces: faces
            .iter()
            .map(|&face| {
                (
                    real_bits(kernel, &GeometryQuery::SurfaceArea(face)),
                    json_real_bits(kernel, &GeometryQuery::Centroid(face), ["x", "y", "z"]),
                )
            })
            .collect(),
        edges: edges
            .iter()
            .map(|&edge| {
                json_real_bits(
                    kernel,
                    &GeometryQuery::BoundingBox(edge),
                    ["xmin", "ymin", "zmin", "xmax", "ymax", "zmax"],
                )
            })
            .collect(),
        vertex_count,
    }
}

fn plate(kernel: &mut OcctKernel) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(PLATE_SIDE),
            height: Value::Real(PLATE_SIDE),
            depth: Value::Real(PLATE_THICKNESS),
        })
        .expect("plate box must succeed")
        .id
}

/// An `n` x `n` grid of plate-piercing cylinders centred on the origin, built
/// as one `LinearPattern2D` so it goes through the single-pass list fuse.
fn hole_grid(kernel: &mut OcctKernel, n: usize, pitch: f64) -> GeometryHandleId {
    let cylinder = kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(HOLE_RADIUS),
            height: Value::Real(2.0 * PLATE_THICKNESS),
        })
        .expect("hole cylinder must succeed")
        .id;
    let grid_origin = -(n as f64 - 1.0) * pitch / 2.0;
    let first_hole = kernel
        .execute(&GeometryOp::Translate {
            target: cylinder,
            dx: grid_origin,
            dy: grid_origin,
            dz: -PLATE_THICKNESS,
        })
        .expect("hole translate must succeed")
        .id;
    kernel
        .execute(&GeometryOp::LinearPattern2D {
            target: first_hole,
            direction1: [1.0, 0.0, 0.0],
            count1: n,
            spacing1: Value::Real(pitch),
            direction2: [0.0, 1.0, 0.0],
            count2: n,
            spacing2: Value::Real(pitch),
        })
        .expect("hole grid pattern must succeed")
        .id
}

/// The top_deck shape in miniature: a plate and an `n` x `n` grid of
/// disjoint holes that pierce it.
fn hole_grid_plate(kernel: &mut OcctKernel, n: usize) -> (GeometryHandleId, GeometryHandleId) {
    (plate(kernel), hole_grid(kernel, n, DISJOINT_PITCH))
}

/// Run `build` on a fresh kernel with booleans in `mode`.
fn run_in<T>(mode: BooleanParallelism, build: impl Fn(&mut OcctKernel) -> T) -> T {
    with_boolean_parallelism(mode, || build(&mut OcctKernel::new()))
}

/// The first line at which the pretty-printed values differ.
fn first_divergence<T: Debug>(serial: &T, parallel: &T) -> String {
    let serial = format!("{serial:#?}");
    let parallel = format!("{parallel:#?}");
    match serial
        .lines()
        .zip(parallel.lines())
        .enumerate()
        .find(|(_, (s, p))| s != p)
    {
        Some((line, (s, p))) => format!("line {line}: serial `{s}` vs parallel `{p}`"),
        None => format!(
            "serial prints {} lines, parallel {}",
            serial.lines().count(),
            parallel.lines().count()
        ),
    }
}

struct SerialVsParallel<T> {
    serial: T,
    /// One entry per Parallel run whose result differs from the Serial one.
    divergences: Vec<String>,
}

/// Run `build` once Serial, then `parallel_runs` times Parallel.
fn serial_vs_parallel<T: Debug + PartialEq>(
    parallel_runs: usize,
    build: impl Fn(&mut OcctKernel) -> T,
) -> SerialVsParallel<T> {
    let serial = run_in(BooleanParallelism::Serial, &build);
    let divergences = (1..=parallel_runs)
        .filter_map(|run| {
            let parallel = run_in(BooleanParallelism::Parallel, &build);
            (parallel != serial).then(|| {
                format!(
                    "parallel run {run}: {}",
                    first_divergence(&serial, &parallel)
                )
            })
        })
        .collect();
    SerialVsParallel {
        serial,
        divergences,
    }
}

#[test]
fn cut_with_history_of_hole_grid_is_identical_serial_and_parallel() {
    const N: usize = 10;
    let outcome = serial_vs_parallel(3, |kernel| {
        let (plate, holes) = hole_grid_plate(kernel, N);
        let (cut, history) = kernel
            .boolean_cut_with_history(plate, holes)
            .expect("plate minus hole grid must succeed");
        (fingerprint(kernel, cut.id), history)
    });
    assert_eq!(
        outcome.serial.0.faces.len(),
        6 + N * N,
        "fixture: every hole must pierce the plate (6 plate faces + one wall per hole)"
    );
    assert!(
        outcome.divergences.is_empty(),
        "parallel cut_with_history diverged from serial:\n{}",
        outcome.divergences.join("\n")
    );
}

type BinaryBoolean = fn(
    &mut OcctKernel,
    GeometryHandleId,
    GeometryHandleId,
) -> (GeometryHandleId, Option<BooleanOpHistoryRecords>);

#[test]
fn every_binary_boolean_is_identical_serial_and_parallel() {
    const N: usize = 6;
    let binary_booleans: [(&str, BinaryBoolean); 6] = [
        ("Union", |kernel, left, right| {
            let result = kernel.execute(&GeometryOp::Union { left, right });
            (result.expect("Union must succeed").id, None)
        }),
        ("Difference", |kernel, left, right| {
            let result = kernel.execute(&GeometryOp::Difference { left, right });
            (result.expect("Difference must succeed").id, None)
        }),
        ("Intersection", |kernel, left, right| {
            let result = kernel.execute(&GeometryOp::Intersection { left, right });
            (result.expect("Intersection must succeed").id, None)
        }),
        ("boolean_fuse_with_history", |kernel, left, right| {
            let (result, history) = kernel
                .boolean_fuse_with_history(left, right)
                .expect("boolean_fuse_with_history must succeed");
            (result.id, Some(history))
        }),
        ("boolean_cut_with_history", |kernel, left, right| {
            let (result, history) = kernel
                .boolean_cut_with_history(left, right)
                .expect("boolean_cut_with_history must succeed");
            (result.id, Some(history))
        }),
        ("boolean_common_with_history", |kernel, left, right| {
            let (result, history) = kernel
                .boolean_common_with_history(left, right)
                .expect("boolean_common_with_history must succeed");
            (result.id, Some(history))
        }),
    ];

    let mut mismatches = Vec::new();
    for (name, run) in binary_booleans {
        let outcome = serial_vs_parallel(2, |kernel| {
            let (plate, holes) = hole_grid_plate(kernel, N);
            let (result, history) = run(kernel, plate, holes);
            (fingerprint(kernel, result), history)
        });
        mismatches.extend(
            outcome
                .divergences
                .into_iter()
                .map(|divergence| format!("  {name}: {divergence}")),
        );
    }
    assert!(
        mismatches.is_empty(),
        "every binary boolean must be identical serial and parallel:\n{}",
        mismatches.join("\n")
    );
}

#[test]
fn overlapping_pattern_fuse_is_identical_serial_and_parallel() {
    let outcome = serial_vs_parallel(2, |kernel| {
        let merged = hole_grid(kernel, 4, OVERLAPPING_PITCH);
        fingerprint(kernel, merged)
    });
    assert_eq!(
        outcome.serial.repr,
        Some(BRepKind::Solid),
        "fixture: overlapping instances must fuse into one solid"
    );
    assert!(
        outcome.divergences.is_empty(),
        "parallel overlapping pattern fuse diverged from serial:\n{}",
        outcome.divergences.join("\n")
    );
}

#[test]
fn split_of_perforated_plate_is_identical_serial_and_parallel() {
    const N: usize = 6;
    let hole_row_y = -(N as f64 - 1.0) * DISJOINT_PITCH / 2.0 + (N / 2) as f64 * DISJOINT_PITCH;
    let outcome = serial_vs_parallel(2, |kernel| {
        let (plate, holes) = hole_grid_plate(kernel, N);
        let perforated = kernel
            .execute(&GeometryOp::Difference {
                left: plate,
                right: holes,
            })
            .expect("plate minus hole grid must succeed")
            .id;
        let pieces = kernel
            .execute_split(&GeometryOp::Split {
                target: perforated,
                plane_origin: [0.0, hole_row_y, 0.0],
                plane_normal: [0.0, 1.0, 0.0],
            })
            .expect("split through a hole row must succeed");
        pieces
            .into_iter()
            .map(|piece| fingerprint(kernel, piece))
            .collect::<Vec<_>>()
    });
    assert_eq!(
        outcome.serial.len(),
        2,
        "fixture: a plane through a hole row must split the plate in two"
    );
    assert!(
        outcome.divergences.is_empty(),
        "parallel split diverged from serial:\n{}",
        outcome.divergences.join("\n")
    );
}

#[test]
fn with_boolean_parallelism_restores_the_previous_mode() {
    let outer = boolean_parallelism();
    with_boolean_parallelism(BooleanParallelism::Serial, || {
        assert_eq!(boolean_parallelism(), BooleanParallelism::Serial);
        with_boolean_parallelism(BooleanParallelism::Parallel, || {
            assert_eq!(boolean_parallelism(), BooleanParallelism::Parallel);
        });
        assert_eq!(
            boolean_parallelism(),
            BooleanParallelism::Serial,
            "leaving the inner scope must restore the outer scope's mode"
        );

        let unwound = std::panic::catch_unwind(|| {
            with_boolean_parallelism(BooleanParallelism::Parallel, || {
                panic!("deliberate panic inside a parallel scope")
            })
        });
        assert!(unwound.is_err(), "the closure's panic must propagate");
        assert_eq!(
            boolean_parallelism(),
            BooleanParallelism::Serial,
            "unwinding out of a scope must restore the prior mode too"
        );
    });
    assert_eq!(
        boolean_parallelism(),
        outer,
        "leaving the outermost scope must restore the thread's original mode"
    );
}
