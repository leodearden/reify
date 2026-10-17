//! OCCT/Manifold parity for the index spaces `AdjacentFaces` and
//! `SharedEdges` answer in (task #9311).
//!
//! Both queries take positions in the kernel's `extract_faces` enumeration
//! and `SharedEdges` returns positions in its `extract_edges` enumeration —
//! that is how the eval-layer consumers map the replies back to handles. The
//! fixture is one 10×20×30 mm box: OCCT's BRep solid, and the same solid's
//! OCCT tessellation ingested into Manifold. Faces are matched across the
//! kernels by their outward direction, read off the bounding boxes so the
//! mapping does not depend on either kernel's `FaceNormal` sign convention.

#![cfg(has_occt)]

mod common;

use std::collections::{BTreeMap, BTreeSet};

use reify_ir::{GeometryHandleId, GeometryKernel, GeometryQuery, QueryError, Value};
use reify_kernel_manifold::ManifoldKernel;

/// A face's outward direction on an axis-aligned box: `(axis, ±1)`.
type Outward = (usize, i8);

/// Largest bbox extent along a face's normal axis that still reads as flat.
/// OCCT's `BRepBndLib::Add` enlarges a face bbox by the shape tolerance
/// (1e-7 m by default) and the Manifold faces are exactly flat, while the
/// smallest extent a box face spans is 10 mm.
const FLAT_TOL: f64 = 1e-6;

/// Per-component agreement of the two kernels' shared-edge bboxes: OCCT's
/// tolerance enlargement (≤ 1e-7 m) plus the f32 rounding of the ingested
/// mesh coordinates (≲ 1e-9 m at 15 mm).
const EDGE_BBOX_TOL: f64 = 1e-6;

const BOX_FACE_COUNT: usize = 6;

/// One kernel's view of the box: its read-only query seam and the handles
/// `extract_faces` / `extract_edges` returned, in their enumeration order.
struct BoxView<'k> {
    kernel: &'static str,
    query: &'k dyn Fn(&GeometryQuery) -> Result<Value, QueryError>,
    body: GeometryHandleId,
    faces: Vec<GeometryHandleId>,
    edges: Vec<GeometryHandleId>,
}

impl BoxView<'_> {
    fn bbox(&self, handle: GeometryHandleId) -> [f64; 6] {
        match (self.query)(&GeometryQuery::BoundingBox(handle)) {
            Ok(Value::String(s)) => parse_bbox(&s),
            other => panic!(
                "[{}] BoundingBox({handle:?}) must return Ok(Value::String(_)); got {other:?}",
                self.kernel
            ),
        }
    }

    /// Outward direction of every face, in `extract_faces` order.
    fn outward_labels(&self) -> Vec<Outward> {
        let body = self.bbox(self.body);
        self.faces
            .iter()
            .map(|&face| outward(self.kernel, self.bbox(face), body))
            .collect()
    }

    fn int_list(&self, query: &GeometryQuery) -> Vec<usize> {
        match (self.query)(query) {
            Ok(value) => int_list(&value),
            Err(e) => panic!("[{}] query({query:?}) must succeed; got {e:?}", self.kernel),
        }
    }

    /// `AdjacentFaces` of `face`, checked against the reply contract:
    /// ascending, distinct, self excluded, inside the face enumeration.
    fn adjacent(&self, face: usize) -> Vec<usize> {
        let neighbours = self.int_list(&GeometryQuery::AdjacentFaces {
            shape: self.body,
            face_index: face,
        });
        assert!(
            neighbours.windows(2).all(|w| w[0] < w[1])
                && !neighbours.contains(&face)
                && neighbours.iter().all(|&n| n < self.faces.len()),
            "[{}] AdjacentFaces({face}) must be ascending, distinct, self-excluded \
             indices into 0..{}; got {neighbours:?}",
            self.kernel,
            self.faces.len(),
        );
        neighbours
    }

    fn shared(&self, face_a: usize, face_b: usize) -> Vec<usize> {
        self.int_list(&GeometryQuery::SharedEdges {
            shape: self.body,
            face_a,
            face_b,
        })
    }

    /// Each face's outward label mapped to its neighbours' labels.
    fn neighbour_labels(&self, labels: &[Outward]) -> BTreeMap<Outward, BTreeSet<Outward>> {
        (0..self.faces.len())
            .map(|face| {
                let neighbours = self.adjacent(face);
                assert_eq!(
                    neighbours.len(),
                    4,
                    "[{}] box face {face} must have exactly 4 adjacent faces; got {neighbours:?}",
                    self.kernel,
                );
                (
                    labels[face],
                    neighbours.iter().map(|&n| labels[n]).collect(),
                )
            })
            .collect()
    }

    /// For every adjacent face pair, keyed by outward labels: the bbox of the
    /// single edge `SharedEdges` reports, looked up through `extract_edges`.
    fn shared_edge_bboxes(&self, labels: &[Outward]) -> BTreeMap<(Outward, Outward), [f64; 6]> {
        let mut out = BTreeMap::new();
        for face in 0..self.faces.len() {
            for neighbour in self.adjacent(face) {
                let shared = self.shared(face, neighbour);
                assert!(
                    shared.len() == 1 && shared[0] < self.edges.len(),
                    "[{}] adjacent box faces {face},{neighbour} must share exactly one edge \
                     indexing extract_edges (0..{}); got {shared:?}",
                    self.kernel,
                    self.edges.len(),
                );
                let edge_bbox = self.bbox(self.edges[shared[0]]);
                out.insert((labels[face], labels[neighbour]), edge_bbox);
            }
        }
        out
    }

    fn face_with_label(&self, labels: &[Outward], label: Outward) -> usize {
        labels
            .iter()
            .position(|&l| l == label)
            .unwrap_or_else(|| panic!("[{}] no face faces {label:?}", self.kernel))
    }
}

/// `(flat axis, side of the body centre)` of a face bbox.
fn outward(kernel: &str, face: [f64; 6], body: [f64; 6]) -> Outward {
    let flat: Vec<usize> = (0..3)
        .filter(|&a| face[a + 3] - face[a] < FLAT_TOL)
        .collect();
    assert_eq!(
        flat.len(),
        1,
        "[{kernel}] a box face bbox must be flat on exactly one axis; got {face:?}",
    );
    let axis = flat[0];
    let face_mid = (face[axis] + face[axis + 3]) / 2.0;
    let body_mid = (body[axis] + body[axis + 3]) / 2.0;
    (axis, if face_mid > body_mid { 1 } else { -1 })
}

/// Every direction except the face's own axis — the neighbours of a box face.
fn expected_neighbour_labels() -> BTreeMap<Outward, BTreeSet<Outward>> {
    all_outward()
        .map(|(axis, sign)| {
            let neighbours = all_outward().filter(|&(a, _)| a != axis).collect();
            ((axis, sign), neighbours)
        })
        .collect()
}

fn all_outward() -> impl Iterator<Item = Outward> {
    (0..3).flat_map(|axis| [(axis, -1), (axis, 1)])
}

/// Parse the `{"xmin":_,…,"zmax":_}` BoundingBox wire string both kernels
/// emit into `[xmin, ymin, zmin, xmax, ymax, zmax]`.
fn parse_bbox(s: &str) -> [f64; 6] {
    const KEYS: [&str; 6] = ["xmin", "ymin", "zmin", "xmax", "ymax", "zmax"];
    let mut out = [f64::NAN; 6];
    for field in s
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .split(',')
    {
        let (key, val) = field
            .split_once(':')
            .unwrap_or_else(|| panic!("malformed bbox field {field:?} in {s:?}"));
        let idx = KEYS
            .iter()
            .position(|k| *k == key.trim().trim_matches('"'))
            .unwrap_or_else(|| panic!("unexpected bbox key {key:?} in {s:?}"));
        out[idx] = val
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("non-numeric bbox value {val:?} in {s:?}"));
    }
    assert!(
        out.iter().all(|c| !c.is_nan()),
        "bbox JSON {s:?} is missing a key"
    );
    out
}

fn int_list(value: &Value) -> Vec<usize> {
    match value {
        Value::List(items) => items
            .iter()
            .map(|item| match item {
                Value::Int(i) => usize::try_from(*i).expect("indices are non-negative"),
                other => panic!("index list must hold Value::Int; got {other:?}"),
            })
            .collect(),
        other => panic!("expected Value::List of indices; got {other:?}"),
    }
}

/// Build the box in both kernels, extract faces and edges, and hand the two
/// read-only views to `check` as `(occt, manifold)`.
fn with_box_on_both_kernels(check: impl FnOnce(&BoxView, &BoxView)) {
    let (mut occt, occt_body) = common::occt_box_solid();
    let occt_faces = occt.extract_faces(occt_body).expect("OCCT extract_faces");
    let occt_edges = occt.extract_edges(occt_body).expect("OCCT extract_edges");

    let mut manifold = ManifoldKernel::new();
    let manifold_body = manifold
        .ingest_mesh(&common::occt_box())
        .expect("Manifold must ingest the OCCT box tessellation")
        .id;
    let manifold_faces = manifold
        .extract_faces(manifold_body)
        .expect("Manifold extract_faces");
    let manifold_edges = manifold
        .extract_edges(manifold_body)
        .expect("Manifold extract_edges");

    let occt_query = |q: &GeometryQuery| occt.query(q);
    let manifold_query = |q: &GeometryQuery| manifold.query(q);
    check(
        &BoxView {
            kernel: "occt",
            query: &occt_query,
            body: occt_body,
            faces: occt_faces,
            edges: occt_edges,
        },
        &BoxView {
            kernel: "manifold",
            query: &manifold_query,
            body: manifold_body,
            faces: manifold_faces,
            edges: manifold_edges,
        },
    );
}

#[test]
fn both_kernels_enumerate_the_six_box_faces_one_per_outward_direction() {
    with_box_on_both_kernels(|occt, manifold| {
        for view in [occt, manifold] {
            assert_eq!(
                view.faces.len(),
                BOX_FACE_COUNT,
                "[{}] the box must enumerate exactly 6 faces",
                view.kernel,
            );
            let labels: BTreeSet<Outward> = view.outward_labels().into_iter().collect();
            assert_eq!(
                labels,
                all_outward().collect::<BTreeSet<_>>(),
                "[{}] the 6 faces must face ±X, ±Y, ±Z once each",
                view.kernel,
            );
        }
    });
}

#[test]
fn adjacent_faces_agree_across_kernels_in_extract_faces_space() {
    with_box_on_both_kernels(|occt, manifold| {
        let occt_map = occt.neighbour_labels(&occt.outward_labels());
        let manifold_map = manifold.neighbour_labels(&manifold.outward_labels());
        assert_eq!(
            occt_map, manifold_map,
            "OCCT and Manifold must report the same face adjacency"
        );
        assert_eq!(
            occt_map,
            expected_neighbour_labels(),
            "each box face must neighbour exactly the 4 faces on the other two axes"
        );
    });
}

#[test]
fn shared_edges_agree_across_kernels_in_extract_edges_space() {
    with_box_on_both_kernels(|occt, manifold| {
        let occt_edges = occt.shared_edge_bboxes(&occt.outward_labels());
        let manifold_edges = manifold.shared_edge_bboxes(&manifold.outward_labels());
        assert_eq!(
            occt_edges.keys().collect::<Vec<_>>(),
            manifold_edges.keys().collect::<Vec<_>>(),
            "both kernels must report shared edges for the same adjacent face pairs",
        );
        for (pair, occt_bbox) in &occt_edges {
            let manifold_bbox = manifold_edges[pair];
            assert!(
                occt_bbox
                    .iter()
                    .zip(manifold_bbox)
                    .all(|(o, m)| (o - m).abs() < EDGE_BBOX_TOL),
                "faces {pair:?} must share the same edge on both kernels: \
                 OCCT bbox {occt_bbox:?} vs Manifold bbox {manifold_bbox:?}",
            );
        }

        for view in [occt, manifold] {
            let labels = view.outward_labels();
            for (face, &(axis, sign)) in labels.iter().enumerate() {
                let opposite = view.face_with_label(&labels, (axis, -sign));
                assert!(
                    view.shared(face, face).is_empty() && view.shared(face, opposite).is_empty(),
                    "[{}] face {face} must share no edge with itself or its opposite face {opposite}",
                    view.kernel,
                );
            }
        }
    });
}

#[test]
fn face_indices_past_the_face_enumeration_are_rejected_on_both_kernels() {
    with_box_on_both_kernels(|occt, manifold| {
        for view in [occt, manifold] {
            for query in [
                GeometryQuery::AdjacentFaces {
                    shape: view.body,
                    face_index: BOX_FACE_COUNT,
                },
                GeometryQuery::SharedEdges {
                    shape: view.body,
                    face_a: BOX_FACE_COUNT,
                    face_b: BOX_FACE_COUNT,
                },
            ] {
                let reply = (view.query)(&query);
                assert!(
                    matches!(reply, Err(QueryError::QueryFailed(_))),
                    "[{}] query({query:?}) must fail with QueryFailed; got {reply:?}",
                    view.kernel,
                );
            }
        }
    });
}
