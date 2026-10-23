//! Every curated-selection entry point maps its selection onto the parent's
//! sub-shapes alike. A foreign sub-shape, or a parent that cannot be
//! enumerated, draws the same text after the entry point's own name from every
//! entry point of one kind: `fillet_edges_with_history`,
//! `chamfer_edges_with_history` and `chamfer_asymmetric_edges_with_history`
//! for edges, `draft_faces` and `shell_solid_faces` for faces. The contract is
//! equality across entry points, not wording; each message names the values
//! it rejects. `chamfer_asymmetric_edges_with_history` reads an empty
//! selection as every edge, so it must agree with the explicit all-edge one.

#![cfg(has_occt)]

use reify_ir::{GeometryError, GeometryHandleId, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::OcctKernel;

const MM: f64 = 1e-3;

/// No kernel in this file mints this many handles.
const UNISSUED: GeometryHandleId = GeometryHandleId(9_999);

/// One curated-selection call's operands. `plane` is draft's neutral plane;
/// no case here reaches it.
struct Call<'a> {
    parent: GeometryHandleId,
    selection: &'a [GeometryHandleId],
    plane: GeometryHandleId,
}

type Entry = (
    &'static str,
    fn(&mut OcctKernel, &Call<'_>) -> Result<(), GeometryError>,
);

fn edge_entries() -> [Entry; 3] {
    [
        ("fillet_edges_with_history", |k, c| {
            k.fillet_edges_with_history(c.parent, MM, c.selection)
                .map(|_| ())
        }),
        ("chamfer_edges_with_history", |k, c| {
            k.chamfer_edges_with_history(c.parent, MM, c.selection)
                .map(|_| ())
        }),
        ("chamfer_asymmetric_edges_with_history", |k, c| {
            k.chamfer_asymmetric_edges_with_history(c.parent, MM, 2.0 * MM, c.selection)
                .map(|_| ())
        }),
    ]
}

fn face_entries() -> [Entry; 2] {
    [
        ("draft_faces", |k, c| {
            k.draft_faces(c.parent, 0.05, c.plane, c.selection)
                .map(|_| ())
        }),
        ("shell_solid_faces", |k, c| {
            k.shell_solid_faces(c.parent, MM, c.selection).map(|_| ())
        }),
    ]
}

/// A 20×10×15 mm parent box and a foreign 5 mm cube in one kernel, with the
/// parent's first edge and face and the cube's.
struct Fixture {
    kernel: OcctKernel,
    parent: GeometryHandleId,
    own_edge: GeometryHandleId,
    own_face: GeometryHandleId,
    foreign_edge: GeometryHandleId,
    foreign_face: GeometryHandleId,
}

fn make_box(kernel: &mut OcctKernel, [w, h, d]: [f64; 3]) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(w * MM),
            height: Value::Real(h * MM),
            depth: Value::Real(d * MM),
        })
        .expect("box should build")
        .id
}

fn fixture() -> Fixture {
    let mut kernel = OcctKernel::new();
    let parent = make_box(&mut kernel, [20.0, 10.0, 15.0]);
    let foreign = make_box(&mut kernel, [5.0, 5.0, 5.0]);
    let own_edge = kernel.extract_edges(parent).expect("parent edges")[0];
    let own_face = kernel.extract_faces(parent).expect("parent faces")[0];
    let foreign_edge = kernel.extract_edges(foreign).expect("foreign edges")[0];
    let foreign_face = kernel.extract_faces(foreign).expect("foreign faces")[0];
    Fixture {
        kernel,
        parent,
        own_edge,
        own_face,
        foreign_edge,
        foreign_face,
    }
}

/// The text after `"{entry}: "` in the `OperationFailed` message `result`
/// carries.
fn rejection(case: &str, entry: &str, result: Result<(), GeometryError>) -> String {
    let message = match result {
        Err(GeometryError::OperationFailed(m)) => m,
        Err(other) => panic!("{case}: {entry} failed with {other:?}, not OperationFailed"),
        Ok(()) => panic!("{case}: {entry} accepted the selection"),
    };
    message
        .strip_prefix(&format!("{entry}: "))
        .unwrap_or_else(|| {
            panic!("{case}: {entry}'s message does not lead with its name: {message:?}")
        })
        .to_owned()
}

/// Every entry rejects `call` naming each of `named`, and all of them alike
/// once their own names are stripped.
fn assert_rejected_alike(
    case: &str,
    kernel: &mut OcctKernel,
    entries: &[Entry],
    call: &Call,
    named: &[GeometryHandleId],
) {
    let texts: Vec<(&str, String)> = entries
        .iter()
        .map(|&(entry, run)| (entry, rejection(case, entry, run(kernel, call))))
        .collect();
    for (entry, text) in &texts {
        for id in named {
            assert!(
                text.contains(&format!("{id:?}")),
                "{case}: {entry}'s rejection does not name {id:?}: {text:?}"
            );
        }
    }
    let (first_entry, first_text) = &texts[0];
    for (entry, text) in &texts[1..] {
        assert_eq!(
            text, first_text,
            "{case}: {entry} and {first_entry} reject differently"
        );
    }
}

#[test]
fn foreign_sub_shape_is_rejected_alike_by_every_curated_entry_point() {
    let mut f = fixture();

    let edges = [f.own_edge, f.foreign_edge];
    let call = Call {
        parent: f.parent,
        selection: &edges,
        plane: f.own_face,
    };
    let named = [f.foreign_edge, f.parent];
    assert_rejected_alike(
        "foreign edge",
        &mut f.kernel,
        &edge_entries(),
        &call,
        &named,
    );

    let faces = [f.own_face, f.foreign_face];
    let call = Call {
        parent: f.parent,
        selection: &faces,
        plane: f.own_face,
    };
    let named = [f.foreign_face, f.parent];
    assert_rejected_alike(
        "foreign face",
        &mut f.kernel,
        &face_entries(),
        &call,
        &named,
    );
}

#[test]
fn unknown_parent_enumeration_failure_is_reported_alike() {
    let mut f = fixture();

    let edges = [f.own_edge];
    let call = Call {
        parent: UNISSUED,
        selection: &edges,
        plane: f.own_face,
    };
    assert_rejected_alike(
        "unknown parent",
        &mut f.kernel,
        &edge_entries(),
        &call,
        &[UNISSUED],
    );

    let faces = [f.own_face];
    let call = Call {
        parent: UNISSUED,
        selection: &faces,
        plane: f.own_face,
    };
    assert_rejected_alike(
        "unknown parent",
        &mut f.kernel,
        &face_entries(),
        &call,
        &[UNISSUED],
    );
}

fn volume_of(kernel: &OcctKernel, id: GeometryHandleId) -> f64 {
    kernel
        .query(&GeometryQuery::Volume(id))
        .expect("volume query should succeed")
        .as_f64()
        .expect("volume should be numeric")
}

#[test]
fn chamfer_asymmetric_empty_selection_equals_explicit_all_edges() {
    let mut f = fixture();
    let setback = 0.5 * MM;

    let (by_empty, empty_records) = f
        .kernel
        .chamfer_asymmetric_edges_with_history(f.parent, setback, setback, &[])
        .expect("an empty selection chamfers every edge");
    let all_edges = f.kernel.extract_edges(f.parent).expect("parent edges");
    let (by_all, all_records) = f
        .kernel
        .chamfer_asymmetric_edges_with_history(f.parent, setback, setback, &all_edges)
        .expect("the explicit all-edge selection chamfers");

    assert_eq!(empty_records, all_records);
    let (empty_volume, all_volume) = (
        volume_of(&f.kernel, by_empty.id),
        volume_of(&f.kernel, by_all.id),
    );
    assert!(
        ((empty_volume - all_volume) / all_volume).abs() <= 1e-12,
        "empty selection volume {empty_volume} vs all-edge {all_volume}"
    );
    assert!(
        all_volume < volume_of(&f.kernel, f.parent),
        "the chamfer removed no material"
    );
}
