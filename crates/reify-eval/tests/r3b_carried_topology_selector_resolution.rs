//! Task 4122 (R3b): kernel-free eval-path selector → node resolver against the
//! carried result topology (`CarriedTopology`, task 4654 R3a).
//!
//! These integration tests exercise the public eval-path API:
//!   - `reify_eval::topology_selectors::resolve_against_carried_topology`
//!     (the kernel-free "carried-topology resolution MODE" of the 4118 executor)
//!   - `reify_eval::topology_selectors::nodes_for_faces` (face → node mapping)
//!
//! and prove PRD §7.2 two-way parity (eval-resolution == live-kernel-resolution
//! node-set) with a `MockGeometryKernel` seeded to mirror the carried topology —
//! NO OCCT in the test binary.

use reify_core::Diagnostic;
use reify_core::identity::RealizationNodeId;
use reify_core::ty::SelectorKind;
use reify_ir::Value;
use reify_ir::boundary_attachment::{BoundaryAssociation, NodeAttachment};
use reify_ir::geometry::{ElementOrderTag, GeometryHandleId, VolumeMesh};
use reify_ir::value::{GeometryHandleRef, LeafQuery, SelectorValue};

use reify_eval::compute_targets::result_topology::{CarriedTopology, from_realized_mesh};

/// One degree expressed in radians — the angular tolerance for the
/// `faces_by_normal` cone in every selector below.
fn one_degree_rad() -> f64 {
    1.0_f64.to_radians()
}

/// A symbolic part reference (kernel_handle: None) matching R3b's selector
/// target — the carried topology carries this identity.
fn symbolic_part() -> GeometryHandleRef {
    GeometryHandleRef {
        realization_ref: RealizationNodeId::new("beam", 0),
        upstream_values_hash: [7u8; 32],
        kernel_handle: None,
    }
}

/// Build the shared fixture: a `CarriedTopology` with two per-face normals
/// (+Z face = id(10), +Y face = id(11)) and a boundary attaching:
///   - node 0 → OnFace(10)
///   - node 1 → OnFace(11)
///   - node 2 → OnFace(10)
///   - node 3 → OnEdge(10)   [edge attachment — excluded from face→node mapping]
///
/// Built via the shared `from_realized_mesh` builder so the test exercises the
/// real R3a construction path, not a hand-rolled struct literal.
fn make_carried() -> CarriedTopology {
    let part = symbolic_part();

    // Flat XYZ for 4 nodes.
    let vertices: Vec<f32> = vec![
        0.0, 0.0, 0.0, // node 0
        1.0, 0.0, 0.0, // node 1
        0.0, 1.0, 0.0, // node 2
        0.0, 0.0, 1.0, // node 3
    ];

    let mut boundary = BoundaryAssociation::default();
    boundary.associate(0, NodeAttachment::OnFace(GeometryHandleId(10)));
    boundary.associate(1, NodeAttachment::OnFace(GeometryHandleId(11)));
    boundary.associate(2, NodeAttachment::OnFace(GeometryHandleId(10)));
    // Edge attachment on the +Z face handle — face→node mapping must EXCLUDE it.
    boundary.associate(3, NodeAttachment::OnEdge(GeometryHandleId(10)));

    let mesh = VolumeMesh {
        vertices,
        tet_indices: vec![0, 1, 2, 3],
        element_order: ElementOrderTag::P1,
        normals: None,
        boundary: Some(boundary),
    };

    // +Z face (id 10) and +Y face (id 11).
    let face_normals = vec![
        (GeometryHandleId(10), [0.0_f64, 0.0, 1.0]),
        (GeometryHandleId(11), [0.0_f64, 1.0, 0.0]),
    ];

    from_realized_mesh(part, &mesh, face_normals)
}

/// Build a `faces_by_normal` leaf selector over the carried symbolic part.
fn normal_leaf(carried: &CarriedTopology, dir: [f64; 3]) -> SelectorValue {
    SelectorValue::leaf(
        SelectorKind::Face,
        carried.part().clone(),
        LeafQuery::ByNormal {
            dir,
            tol_rad: one_degree_rad(),
        },
    )
    .expect("Face kind matches ByNormal required kind")
}

/// Sort the resolved handle ids for order-independent set comparison.
fn sorted_ids(v: Vec<GeometryHandleId>) -> Vec<u64> {
    let mut ids: Vec<u64> = v.into_iter().map(|h| h.0).collect();
    ids.sort_unstable();
    ids
}

/// Sort the resolved node indices for order-independent set comparison.
fn sorted_usize(mut v: Vec<usize>) -> Vec<usize> {
    v.sort_unstable();
    v
}

// ── step-01 tests (RED until step-02: resolve_against_carried_topology) ───────

/// (a) The +Z selector resolves to exactly {id(10)} against the carried
/// topology — kernel-free, reading only `face_normals()`.
#[test]
fn plus_z_resolves_to_single_face() {
    let carried = make_carried();
    let sel = normal_leaf(&carried, [0.0, 0.0, 1.0]);

    let resolved =
        reify_eval::topology_selectors::resolve_against_carried_topology(&sel, &carried)
            .expect("ByNormal leaf resolves against carried topology");

    assert_eq!(
        sorted_ids(resolved),
        vec![10],
        "+Z normal selects only the +Z face id(10)"
    );
}

/// (b) Direction sensitivity: a -Z direction (anti-parallel to id(10)) and a +X
/// direction (perpendicular to both faces) each resolve to the EMPTY set —
/// matching the live `faces_by_normal` predicate (anti-parallel excluded).
#[test]
fn anti_parallel_and_perpendicular_resolve_empty() {
    let carried = make_carried();

    // -Z is 180° from id(10)'s +Z normal → excluded (not 0° within 1°).
    let neg_z = normal_leaf(&carried, [0.0, 0.0, -1.0]);
    let resolved_neg_z =
        reify_eval::topology_selectors::resolve_against_carried_topology(&neg_z, &carried)
            .expect("resolves");
    assert!(
        sorted_ids(resolved_neg_z).is_empty(),
        "-Z is anti-parallel to id(10) and perpendicular to id(11): empty"
    );

    // +X is 90° from both face normals → excluded.
    let plus_x = normal_leaf(&carried, [1.0, 0.0, 0.0]);
    let resolved_plus_x =
        reify_eval::topology_selectors::resolve_against_carried_topology(&plus_x, &carried)
            .expect("resolves");
    assert!(
        sorted_ids(resolved_plus_x).is_empty(),
        "+X is perpendicular to both faces: empty"
    );
}

/// (c) Set composition: a Union of the +Z and +Y leaves resolves to {10, 11};
/// a Difference of that union minus the +Y leaf removes the second set → {10}.
#[test]
fn union_and_difference_compose() {
    let carried = make_carried();

    let plus_z = normal_leaf(&carried, [0.0, 0.0, 1.0]);
    let plus_y = normal_leaf(&carried, [0.0, 1.0, 0.0]);

    let union = SelectorValue::union(vec![plus_z.clone(), plus_y.clone()])
        .expect("same-kind union");
    let resolved_union =
        reify_eval::topology_selectors::resolve_against_carried_topology(&union, &carried)
            .expect("resolves");
    assert_eq!(
        sorted_ids(resolved_union),
        vec![10, 11],
        "Union of +Z and +Y selects both faces"
    );

    // Difference: (+Z ∪ +Y) minus (+Y leaf) removes id(11), leaving {10}.
    let difference = SelectorValue::difference(union, plus_y).expect("same-kind difference");
    let resolved_diff =
        reify_eval::topology_selectors::resolve_against_carried_topology(&difference, &carried)
            .expect("resolves");
    assert_eq!(
        sorted_ids(resolved_diff),
        vec![10],
        "Difference removes the +Y face id(11)"
    );
}

// ── step-03 tests (RED until step-04: nodes_for_faces) ────────────────────────

/// The +Z face set {id(10)} maps to its OnFace node-set {0, 2}; the OnEdge(10)
/// attachment of node 3 is EXCLUDED (only OnFace attachments contribute nodes).
#[test]
fn nodes_for_faces_excludes_edge_attachments() {
    let carried = make_carried();
    let nodes =
        reify_eval::topology_selectors::nodes_for_faces(&[GeometryHandleId(10)], &carried);
    assert_eq!(
        sorted_usize(nodes),
        vec![0, 2],
        "id(10) OnFace nodes are {{0,2}}; node 3 OnEdge(10) is excluded"
    );
}

/// The face set {id(10), id(11)} maps to the union of their OnFace nodes
/// {0, 1, 2} (sorted, deduped).
#[test]
fn nodes_for_faces_unions_multiple_faces() {
    let carried = make_carried();
    let nodes = reify_eval::topology_selectors::nodes_for_faces(
        &[GeometryHandleId(10), GeometryHandleId(11)],
        &carried,
    );
    assert_eq!(
        sorted_usize(nodes),
        vec![0, 1, 2],
        "OnFace(10)={{0,2}} ∪ OnFace(11)={{1}} = {{0,1,2}}"
    );
}

/// A face handle not present in the boundary maps to the empty node-set
/// (no panic, honest empty).
#[test]
fn nodes_for_faces_unknown_face_is_empty() {
    let carried = make_carried();
    let nodes =
        reify_eval::topology_selectors::nodes_for_faces(&[GeometryHandleId(999)], &carried);
    assert!(
        nodes.is_empty(),
        "a face handle absent from the boundary maps to no nodes"
    );
}

// ── step-05 test: PRD §7.2 two-way boundary parity (kernel-free, GREEN on arrival) ──

/// The H acceptance bar (PRD §7.2): a `faces_by_normal(+Z)` selector resolves to
/// the SAME face-set and the SAME node-set whether resolved against the LIVE
/// kernel (a `MockGeometryKernel` seeded to MIRROR the carried topology) or
/// against the carried topology (kernel-free) — proving the shared predicate
/// helpers preserve parity by construction. Exact discrete set equality (no
/// float tolerance on any magnitude — numeric-floor item c). NO OCCT in this
/// binary.
#[test]
fn two_way_kernel_vs_carried_parity() {
    let carried = make_carried();
    let mut diags: Vec<Diagnostic> = Vec::new();

    // ── Seed a MockGeometryKernel to MIRROR the carried topology ──────────────
    // parent solid → the two faces; each face's FaceNormal is the SAME normal
    // carried.face_normals() bakes (encoded as the kernel's {"x","y","z"} JSON).
    let parent = GeometryHandleId(1);
    let mut mock = reify_test_support::MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![GeometryHandleId(10), GeometryHandleId(11)])
        .with_face_normal_result(
            GeometryHandleId(10),
            Value::String("{\"x\":0,\"y\":0,\"z\":1}".into()),
        )
        .with_face_normal_result(
            GeometryHandleId(11),
            Value::String("{\"x\":0,\"y\":1,\"z\":0}".into()),
        );

    // ── Same faces_by_normal(+Z) selector, two targets ────────────────────────
    // Live-kernel path: target carries kernel_handle = Some(parent).
    let kernel_target = GeometryHandleRef {
        realization_ref: carried.part().realization_ref.clone(),
        upstream_values_hash: carried.part().upstream_values_hash,
        kernel_handle: Some(parent),
    };
    let sel_kernel = SelectorValue::leaf(
        SelectorKind::Face,
        kernel_target,
        LeafQuery::ByNormal {
            dir: [0.0, 0.0, 1.0],
            tol_rad: one_degree_rad(),
        },
    )
    .expect("kernel selector");
    // Carried path: symbolic target (kernel_handle None).
    let sel_symbolic = normal_leaf(&carried, [0.0, 0.0, 1.0]);

    // ── Resolve both ways ─────────────────────────────────────────────────────
    let kernel_faces =
        reify_eval::topology_selectors::resolve(&sel_kernel, &mut mock, &mut diags)
            .expect("live-kernel resolution");
    let carried_faces = reify_eval::topology_selectors::resolve_against_carried_topology(
        &sel_symbolic,
        &carried,
    )
    .expect("carried resolution");

    // ── Face-handle SET parity ────────────────────────────────────────────────
    assert_eq!(
        sorted_ids(kernel_faces.clone()),
        sorted_ids(carried_faces.clone()),
        "PRD §7.2: live-kernel and carried face-sets must be identical"
    );
    assert_eq!(
        sorted_ids(kernel_faces.clone()),
        vec![10],
        "+Z resolves to the +Z face on both paths"
    );

    // ── NODE-SET parity through the shared boundary ───────────────────────────
    let kernel_nodes = reify_eval::topology_selectors::nodes_for_faces(&kernel_faces, &carried);
    let carried_nodes =
        reify_eval::topology_selectors::nodes_for_faces(&carried_faces, &carried);
    assert_eq!(
        sorted_usize(kernel_nodes),
        sorted_usize(carried_nodes),
        "PRD §7.2: node-sets mapped through the shared boundary must be identical"
    );
}
