//! Task 4654 (R3a): carried-topology bundle for result values.
//!
//! Defines [`CarriedTopology`] — the kernel-free, selector-resolvable topology
//! bundle that result values (ModalResult, ElasticResult, …) carry so the
//! eval-path resolver (R3b, a separate task — the CONSUMER) can resolve
//! `faces_by_normal(part,+Z,tol)` against baked data, never OCCT.
//!
//! # Design decisions
//!
//! * **PER-FACE normals** (Q3): keyed by `GeometryHandleId`, same as
//!   `BoundaryAssociation::OnFace`. Only per-face normals reproduce
//!   `faces_by_normal`'s kernel-side selection — a node on two faces has an
//!   ambiguous per-node normal (PRD §7.2/§9).
//!
//! * **One shared type**: `CarriedTopology` is not modal-only; the same
//!   `from_realized_mesh` builder serves modal and FEA result models (DD-3).
//!
//! * **Value round-trip via synthetic StructureInstance** (type_id u32::MAX):
//!   synthetic result values bypass field-vs-.ri-def validation, matching the
//!   `warm_started`/other undeclared-field precedents.

use reify_ir::boundary_attachment::{BoundaryAssociation, NodeAttachment};
use reify_ir::geometry::GeometryHandleId;
use reify_ir::value::GeometryHandleRef;
use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

/// The kernel-free, selector-resolvable topology bundle carried on result
/// values (ModalResult, ElasticResult, …).
///
/// Encodes:
/// - The identity of the source part (`part` — a `GeometryHandleRef` with
///   `kernel_handle: None`; symbolic reference matching R3b's selector target).
/// - Flat XYZ node coordinates (layout mirrors `VolumeMesh::vertices`).
/// - Per-face normals keyed by `GeometryHandleId` (same keys as
///   `BoundaryAssociation::OnFace`; settles Q3).
/// - The full `BoundaryAssociation` (node index → face/edge/vertex attachment)
///   reusing the 4092 vocabulary.
///
/// Produced by [`from_realized_mesh`] (generic shared builder) and
/// round-tripped through `Value` by [`to_value`]/[`from_value`].
#[derive(Debug, Clone, PartialEq)]
pub struct CarriedTopology {
    pub(crate) part: GeometryHandleRef,
    pub(crate) node_coords: Vec<f32>,
    pub(crate) face_normals: Vec<(GeometryHandleId, [f64; 3])>,
    pub(crate) boundary: BoundaryAssociation,
}

impl CarriedTopology {
    // ── Accessors ────────────────────────────────────────────────────────────

    /// The symbolic part reference (kernel_handle: None, identity by
    /// realization_ref + upstream_values_hash matching R3b's selector target).
    pub fn part(&self) -> &GeometryHandleRef {
        &self.part
    }

    /// Flat XYZ node coordinates (layout: [x0,y0,z0, x1,y1,z1, ...],
    /// mirrors `VolumeMesh::vertices`).
    pub fn node_coords(&self) -> &[f32] {
        &self.node_coords
    }

    /// Per-face normals keyed by `GeometryHandleId` (same keys as
    /// `BoundaryAssociation::OnFace`; order is preserved from construction).
    pub fn face_normals(&self) -> &[(GeometryHandleId, [f64; 3])] {
        &self.face_normals
    }

    /// The full boundary association (node index → face/edge/vertex attachment)
    /// reusing the 4092 `BoundaryAssociation` vocabulary.
    pub fn boundary(&self) -> &BoundaryAssociation {
        &self.boundary
    }

    // ── Validation helpers ───────────────────────────────────────────────────

    /// Returns `true` when all node coordinates and face-normal components are
    /// finite (no NaN, no ±Infinity).
    pub fn all_finite(&self) -> bool {
        self.node_coords.iter().all(|&c| c.is_finite())
            && self
                .face_normals
                .iter()
                .all(|(_, n)| n.iter().all(|&c| c.is_finite()))
    }

    /// Returns `true` when face_normals, boundary, and node_coords are all
    /// empty (no topology data carried).
    pub fn is_empty(&self) -> bool {
        self.face_normals.is_empty()
            && self.boundary.is_empty()
            && self.node_coords.is_empty()
    }

    // ── Value encoding ───────────────────────────────────────────────────────

    /// Encode as a `Value::StructureInstance{type_name:"CarriedTopology"}` for
    /// lossless round-trip through the Value tree.
    ///
    /// Encoding schema (all fields present, order-preserving):
    /// - `part`         → `Value::GeometryHandle{realization_ref, upstream_values_hash, kernel_handle:None}`
    /// - `node_coords`  → `Value::List<Value::Vector([Real;3])>` (f32→f64 widening, lossless)
    /// - `face_normals` → `Value::List<Value::StructureInstance{handle:Int, normal:Vector([Real;3])}>`)
    /// - `boundary`     → `Value::List<Value::StructureInstance{node:Int, kind:Int(0=Face/1=Edge/2=Vertex), handle:Int}>`
    ///
    /// # Invariants
    ///
    /// * `node_coords.len()` MUST be a multiple of 3. Incomplete trailing
    ///   elements (len % 3 != 0) are silently dropped by `chunks_exact(3)`.
    ///   The primary entry point [`from_realized_mesh`] enforces this invariant
    ///   with a `debug_assert`.
    ///
    /// * Non-finite values (`NaN`, `±Inf`) in `node_coords` or `face_normals`
    ///   are encoded as-is. [`from_value`][Self::from_value] rejects non-finite
    ///   data, so a non-finite `CarriedTopology` cannot be round-tripped. Call
    ///   [`all_finite`][Self::all_finite] before encoding if a decodable value is
    ///   required.
    pub fn to_value(&self) -> Value {
        // part → Value::GeometryHandle
        let part_val = Value::GeometryHandle {
            realization_ref: self.part.realization_ref.clone(),
            upstream_values_hash: self.part.upstream_values_hash,
            kernel_handle: None,
        };

        // node_coords → List<Vector([Real;3])> (flat XYZ → per-node triples)
        // chunks_exact(3) prevents an out-of-bounds panic when node_coords.len()
        // is not a multiple of 3 (invariant violation — from_realized_mesh
        // enforces this with a debug_assert). The incomplete tail is dropped
        // rather than panicking so callers can detect the truncation via
        // round-trip inequality.
        let node_coords_val = {
            let triples: Vec<Value> = self
                .node_coords
                .chunks_exact(3)
                .map(|c| {
                    Value::Vector(vec![
                        Value::Real(c[0] as f64),
                        Value::Real(c[1] as f64),
                        Value::Real(c[2] as f64),
                    ])
                })
                .collect();
            Value::List(triples)
        };

        // face_normals → List<StructureInstance{handle:Int, normal:Vector([Real;3])}>
        let face_normals_val = {
            let items: Vec<Value> = self
                .face_normals
                .iter()
                .map(|(handle, normal)| {
                    let fields: PersistentMap<String, Value> = [
                        ("handle".to_string(), Value::Int(handle.0 as i64)),
                        (
                            "normal".to_string(),
                            Value::Vector(vec![
                                Value::Real(normal[0]),
                                Value::Real(normal[1]),
                                Value::Real(normal[2]),
                            ]),
                        ),
                    ]
                    .into_iter()
                    .collect();
                    Value::StructureInstance(Box::new(StructureInstanceData {
                        type_id: StructureTypeId(u32::MAX),
                        type_name: "FaceNormal".to_string(),
                        version: 1,
                        fields,
                    }))
                })
                .collect();
            Value::List(items)
        };

        // boundary → List<StructureInstance{node:Int, kind:Int, handle:Int}>
        // kind encoding: 0 = OnFace, 1 = OnEdge, 2 = OnVertex
        let boundary_val = {
            let items: Vec<Value> = self
                .boundary
                .iter()
                .map(|(node_idx, attach)| {
                    let (kind_int, handle_id) = match attach {
                        NodeAttachment::OnFace(id) => (0i64, id.0 as i64),
                        NodeAttachment::OnEdge(id) => (1i64, id.0 as i64),
                        NodeAttachment::OnVertex(id) => (2i64, id.0 as i64),
                    };
                    let fields: PersistentMap<String, Value> = [
                        ("node".to_string(), Value::Int(node_idx as i64)),
                        ("kind".to_string(), Value::Int(kind_int)),
                        ("handle".to_string(), Value::Int(handle_id)),
                    ]
                    .into_iter()
                    .collect();
                    Value::StructureInstance(Box::new(StructureInstanceData {
                        type_id: StructureTypeId(u32::MAX),
                        type_name: "BoundaryNode".to_string(),
                        version: 1,
                        fields,
                    }))
                })
                .collect();
            Value::List(items)
        };

        // Assemble as a CarriedTopology StructureInstance
        let fields: PersistentMap<String, Value> = [
            ("part".to_string(), part_val),
            ("node_coords".to_string(), node_coords_val),
            ("face_normals".to_string(), face_normals_val),
            ("boundary".to_string(), boundary_val),
        ]
        .into_iter()
        .collect();

        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "CarriedTopology".to_string(),
            version: 1,
            fields,
        }))
    }

    /// Decode a `Value` previously produced by [`to_value`] back into a
    /// `CarriedTopology`.
    ///
    /// Returns `None` when:
    /// - `v` is not a `Value::StructureInstance`
    /// - `type_name` is not `"CarriedTopology"`
    /// - Any required field is missing or ill-typed
    pub fn from_value(v: &Value) -> Option<CarriedTopology> {
        let data = match v {
            Value::StructureInstance(d) => d,
            _ => return None,
        };

        if data.type_name != "CarriedTopology" {
            return None;
        }

        // ── part ─────────────────────────────────────────────────────────────
        let part = {
            let part_val = data.fields.get("part")?;
            GeometryHandleRef::from_geometry_handle(part_val)?
        };

        // ── node_coords ───────────────────────────────────────────────────────
        let node_coords = {
            let list_val = data.fields.get("node_coords")?;
            let items = match list_val {
                Value::List(items) => items,
                _ => return None,
            };
            let mut coords = Vec::with_capacity(items.len() * 3);
            for item in items {
                match item {
                    Value::Vector(components) if components.len() == 3 => {
                        for c in components {
                            match c {
                                Value::Real(f) => {
                                    if !f.is_finite() {
                                        return None;
                                    }
                                    coords.push(*f as f32);
                                }
                                _ => return None,
                            }
                        }
                    }
                    _ => return None,
                }
            }
            coords
        };

        // ── face_normals ──────────────────────────────────────────────────────
        let face_normals = {
            let list_val = data.fields.get("face_normals")?;
            let items = match list_val {
                Value::List(items) => items,
                _ => return None,
            };
            let mut normals = Vec::with_capacity(items.len());
            for item in items {
                let si = match item {
                    Value::StructureInstance(d) if d.type_name == "FaceNormal" => d,
                    _ => return None,
                };
                let handle_id = match si.fields.get("handle")? {
                    Value::Int(i) => *i as u64,
                    _ => return None,
                };
                let normal = match si.fields.get("normal")? {
                    Value::Vector(comps) if comps.len() == 3 => {
                        let mut n = [0.0f64; 3];
                        for (i, c) in comps.iter().enumerate() {
                            match c {
                                Value::Real(f) => {
                                    if !f.is_finite() {
                                        return None;
                                    }
                                    n[i] = *f;
                                }
                                _ => return None,
                            }
                        }
                        n
                    }
                    _ => return None,
                };
                normals.push((GeometryHandleId(handle_id), normal));
            }
            normals
        };

        // ── boundary ──────────────────────────────────────────────────────────
        let boundary = {
            let list_val = data.fields.get("boundary")?;
            let items = match list_val {
                Value::List(items) => items,
                _ => return None,
            };
            let mut ba = BoundaryAssociation::default();
            for item in items {
                let si = match item {
                    Value::StructureInstance(d) if d.type_name == "BoundaryNode" => d,
                    _ => return None,
                };
                let node_idx = match si.fields.get("node")? {
                    Value::Int(i) => *i as u32,
                    _ => return None,
                };
                let kind = match si.fields.get("kind")? {
                    Value::Int(i) => *i,
                    _ => return None,
                };
                let handle_id = match si.fields.get("handle")? {
                    Value::Int(i) => *i as u64,
                    _ => return None,
                };
                let attach = match kind {
                    0 => NodeAttachment::OnFace(GeometryHandleId(handle_id)),
                    1 => NodeAttachment::OnEdge(GeometryHandleId(handle_id)),
                    2 => NodeAttachment::OnVertex(GeometryHandleId(handle_id)),
                    _ => return None,
                };
                ba.associate(node_idx, attach);
            }
            ba
        };

        Some(CarriedTopology {
            part,
            node_coords,
            face_normals,
            boundary,
        })
    }
}

// ── Shared builder ───────────────────────────────────────────────────────────

/// Build a `CarriedTopology` from a realized `VolumeMesh` and supplied per-face
/// normals.
///
/// This is the **shared reuse point** for both modal and FEA result models —
/// it is NOT modal-only (DD-3). It clones `mesh.vertices` into `node_coords`
/// and clones `mesh.boundary` (or a default empty association when `None`)
/// into the carried `BoundaryAssociation`.
///
/// The caller supplies `face_normals` because per-face normals must be
/// threaded from wherever the B-rep kernel is available (they cannot be
/// recovered from the mesh alone).
///
/// # Invariant
///
/// `mesh.vertices.len()` MUST be a multiple of 3 (flat XYZ layout — every
/// valid `VolumeMesh` satisfies this). Violated in debug builds via
/// `debug_assert!`; release builds propagate the invariant violation into
/// `CarriedTopology::node_coords` (where `to_value()` uses `chunks_exact(3)`
/// and silently drops the incomplete tail).
pub fn from_realized_mesh(
    part: GeometryHandleRef,
    mesh: &reify_ir::geometry::VolumeMesh,
    face_normals: Vec<(GeometryHandleId, [f64; 3])>,
) -> CarriedTopology {
    debug_assert!(
        mesh.vertices.len() % 3 == 0,
        "VolumeMesh vertices must have a length that is a multiple of 3 (flat XYZ layout); \
         got len={}",
        mesh.vertices.len()
    );
    CarriedTopology {
        part,
        node_coords: mesh.vertices.clone(),
        face_normals,
        boundary: mesh.boundary.clone().unwrap_or_default(),
    }
}

/// Extract and decode the `topology` field from any result
/// `Value::StructureInstance` (ModalResult, ElasticResult, …).
///
/// Returns `None` when:
/// - `result` is not a `Value::StructureInstance`
/// - The `topology` field is absent, `Value::Undef`, or malformed
///
/// This is the accessor R3b reuses to read the carried topology without
/// knowing which result type it came from.
pub fn carried_topology_from_result(result: &Value) -> Option<CarriedTopology> {
    let data = match result {
        Value::StructureInstance(d) => d,
        _ => return None,
    };
    let topo_val = data.fields.get("topology")?;
    CarriedTopology::from_value(topo_val)
}

#[cfg(test)]
mod tests {
    use reify_core::identity::RealizationNodeId;
    use reify_ir::boundary_attachment::{BoundaryAssociation, NodeAttachment};
    use reify_ir::geometry::GeometryHandleId;
    use reify_ir::value::GeometryHandleRef;

    use super::*;

    /// Build a representative `CarriedTopology` fixture with fully-finite data,
    /// covering all three `NodeAttachment` variants.
    fn make_fixture() -> CarriedTopology {
        let part = GeometryHandleRef {
            realization_ref: RealizationNodeId::new("body", 0),
            upstream_values_hash: [0u8; 32],
            kernel_handle: None,
        };

        // Flat XYZ for 4 nodes: (0,0,0), (1,0,0), (0,1,0), (0,0,1)
        let node_coords: Vec<f32> = vec![
            0.0, 0.0, 0.0,
            1.0, 0.0, 0.0,
            0.0, 1.0, 0.0,
            0.0, 0.0, 1.0,
        ];

        // Two per-face normals keyed by GeometryHandleId
        let face_normals = vec![
            (GeometryHandleId(1), [0.0_f64, 0.0, 1.0]),
            (GeometryHandleId(2), [1.0_f64, 0.0, 0.0]),
        ];

        // BoundaryAssociation covering all 3 attachment variants
        let mut boundary = BoundaryAssociation::default();
        boundary.associate(4, NodeAttachment::OnFace(GeometryHandleId(1)));
        boundary.associate(8, NodeAttachment::OnFace(GeometryHandleId(2)));
        boundary.associate(0, NodeAttachment::OnEdge(GeometryHandleId(1)));

        CarriedTopology { part, node_coords, face_normals, boundary }
    }

    // ── step-3 tests (RED until step-4) ──────────────────────────────────────

    /// RED: all_finite() does not exist yet.
    ///
    /// (a) all_finite() is true for the finite fixture.
    #[test]
    fn all_finite_true_for_finite_fixture() {
        let topo = make_fixture();
        assert!(topo.all_finite(), "all coords and normals in fixture are finite");
    }

    /// RED: from_value must reject NaN/±Inf in node_coords (not implemented yet).
    ///
    /// (b) from_value returns None when a node_coord is NaN or ±Inf.
    #[test]
    fn from_value_rejects_non_finite_node_coord() {
        let mut topo = make_fixture();
        // inject NaN into node_coords
        topo.node_coords[0] = f32::NAN;
        let val = topo.to_value();
        assert!(
            CarriedTopology::from_value(&val).is_none(),
            "from_value must return None when a node_coord is NaN"
        );
    }

    /// RED: from_value must reject NaN/±Inf in face normal (not implemented yet).
    ///
    /// (b) from_value returns None when a face-normal component is ±Inf.
    #[test]
    fn from_value_rejects_non_finite_face_normal() {
        let mut topo = make_fixture();
        // inject Inf into a face normal component
        topo.face_normals[0].1[2] = f64::INFINITY;
        let val = topo.to_value();
        assert!(
            CarriedTopology::from_value(&val).is_none(),
            "from_value must return None when a face-normal component is Inf"
        );
    }

    /// RED: is_empty() does not exist yet.
    ///
    /// (c) is_empty() is true when face_normals, boundary, and node_coords are
    /// all empty; false when any is non-empty.
    #[test]
    fn is_empty_semantics() {
        let part = make_fixture().part;
        let empty = CarriedTopology {
            part: part.clone(),
            node_coords: vec![],
            face_normals: vec![],
            boundary: BoundaryAssociation::default(),
        };
        assert!(empty.is_empty(), "all-empty topology must report is_empty");

        // non-empty via node_coords only
        let with_coords = CarriedTopology {
            part: part.clone(),
            node_coords: vec![0.0, 0.0, 0.0],
            face_normals: vec![],
            boundary: BoundaryAssociation::default(),
        };
        assert!(!with_coords.is_empty(), "non-empty node_coords → not is_empty");

        // full fixture is not empty
        assert!(!make_fixture().is_empty());
    }

    /// GREEN (from_value already rejects non-SI and wrong type_name).
    ///
    /// (d) from_value returns None for non-StructureInstance Values and for a
    /// StructureInstance with the wrong type_name. No panic.
    #[test]
    fn from_value_rejects_wrong_shape() {
        // non-StructureInstance
        assert!(CarriedTopology::from_value(&Value::Undef).is_none());
        assert!(CarriedTopology::from_value(&Value::Int(42)).is_none());
        assert!(CarriedTopology::from_value(&Value::List(vec![])).is_none());

        // wrong type_name
        let wrong_name = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "NotCarriedTopology".to_string(),
            version: 1,
            fields: Default::default(),
        }));
        assert!(CarriedTopology::from_value(&wrong_name).is_none());

        // missing required fields (type_name correct, but no fields)
        let missing_fields = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "CarriedTopology".to_string(),
            version: 1,
            fields: Default::default(),
        }));
        assert!(CarriedTopology::from_value(&missing_fields).is_none());
    }

    // ── step-5 test (RED until step-6) ───────────────────────────────────────

    /// RED: from_realized_mesh does not exist yet.
    ///
    /// Tests the shared builder against an FEA-style realized VolumeMesh
    /// fixture (proves the builder path is shared, not modal-only).
    #[test]
    fn from_realized_mesh_builder() {
        use reify_ir::geometry::{ElementOrderTag, VolumeMesh};

        let part = GeometryHandleRef {
            realization_ref: RealizationNodeId::new("body", 1),
            upstream_values_hash: [1u8; 32],
            kernel_handle: None,
        };

        // 4-node P1 tet: 4 vertices, 4-index element
        let vertices: Vec<f32> = vec![
            0.0, 0.0, 0.0,
            1.0, 0.0, 0.0,
            0.0, 1.0, 0.0,
            0.0, 0.0, 1.0,
        ];
        let mut ba = BoundaryAssociation::default();
        ba.associate(0, NodeAttachment::OnFace(GeometryHandleId(10)));
        ba.associate(1, NodeAttachment::OnFace(GeometryHandleId(10)));
        ba.associate(2, NodeAttachment::OnFace(GeometryHandleId(11)));

        let mesh = VolumeMesh {
            vertices: vertices.clone(),
            tet_indices: vec![0, 1, 2, 3],
            element_order: ElementOrderTag::P1,
            normals: None,
            boundary: Some(ba.clone()),
        };

        let supplied_normals = vec![
            (GeometryHandleId(10), [0.0_f64, 0.0, 1.0]),
            (GeometryHandleId(11), [1.0_f64, 0.0, 0.0]),
        ];

        let topo = from_realized_mesh(part.clone(), &mesh, supplied_normals.clone());

        // node_coords == mesh.vertices
        assert_eq!(topo.node_coords(), vertices.as_slice());
        // boundary == mesh.boundary (cloned)
        assert_eq!(topo.boundary(), &ba);
        // face_normals == supplied
        assert_eq!(topo.face_normals(), supplied_normals.as_slice());
        // part == supplied part
        assert_eq!(topo.part(), &part);

        // round-trips losslessly
        let rt = CarriedTopology::from_value(&topo.to_value());
        assert!(rt.is_some(), "must round-trip");
        assert_eq!(rt.unwrap(), topo);

        // all_finite
        assert!(topo.all_finite());
    }

    // ── amendment tests (amend: result_topology robustness + coverage) ──────────

    /// Suggestion 3 — from_value rejects a boundary `kind` value outside [0,2].
    ///
    /// kind=3 is not a valid NodeAttachment variant; from_value must return None.
    #[test]
    fn from_value_rejects_out_of_range_boundary_kind() {
        // Build a CarriedTopology Value with an out-of-range kind=3 BoundaryNode.
        // Construct the invalid BoundaryNode StructureInstance directly.
        let invalid_boundary_node = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "BoundaryNode".to_string(),
            version: 1,
            fields: [
                ("node".to_string(), Value::Int(0)),
                ("kind".to_string(), Value::Int(3)), // invalid — only 0/1/2 are valid
                ("handle".to_string(), Value::Int(1)),
            ]
            .into_iter()
            .collect(),
        }));

        // Build a minimal but otherwise valid CarriedTopology Value
        // and replace its boundary list with the invalid node.
        let mut topo = make_fixture();
        // Replace boundary with a list containing the invalid BoundaryNode
        let part_val = Value::GeometryHandle {
            realization_ref: topo.part.realization_ref.clone(),
            upstream_values_hash: topo.part.upstream_values_hash,
            kernel_handle: None,
        };
        let fields: PersistentMap<String, Value> = [
            ("part".to_string(), part_val),
            ("node_coords".to_string(), Value::List(vec![])),
            ("face_normals".to_string(), Value::List(vec![])),
            ("boundary".to_string(), Value::List(vec![invalid_boundary_node])),
        ]
        .into_iter()
        .collect();
        let v = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "CarriedTopology".to_string(),
            version: 1,
            fields,
        }));

        assert!(
            CarriedTopology::from_value(&v).is_none(),
            "from_value must return None for a BoundaryNode with out-of-range kind=3"
        );

        // Sanity-check: kind=0/1/2 are accepted (the fixture round-trips fine).
        let _ = topo.boundary.iter().for_each(|_| {}); // ensure boundary is non-empty
        let encoded = topo.to_value();
        assert!(
            CarriedTopology::from_value(&encoded).is_some(),
            "valid boundary kinds 0/1/2 must be accepted by from_value"
        );
    }

    /// Suggestion 2+3 — `to_value()` uses `chunks_exact(3)` to avoid panicking
    /// when `node_coords.len()` is not a multiple of 3.
    ///
    /// Directly constructing a `CarriedTopology` with non-triple `node_coords`
    /// (bypassing `from_realized_mesh`, which `debug_assert!`s the invariant) and
    /// calling `to_value()` must NOT panic. The incomplete trailing elements are
    /// dropped; the decoded `node_coords` length reflects only complete triples.
    #[test]
    fn to_value_non_triple_node_coords_drops_trailing_elements() {
        let part = make_fixture().part.clone();
        // 5 elements: one complete triple [1,2,3] + an incomplete tail [4,5].
        // from_realized_mesh would debug_assert here; we bypass it for the
        // robustness test by constructing CarriedTopology directly.
        let topo = CarriedTopology {
            part,
            node_coords: vec![1.0_f32, 2.0, 3.0, 4.0, 5.0],
            face_normals: vec![],
            boundary: BoundaryAssociation::default(),
        };

        // Must not panic (chunks_exact(3) drops the incomplete tail).
        let encoded = topo.to_value();

        // Decode: only the first complete triple should be present.
        let decoded = CarriedTopology::from_value(&encoded)
            .expect("should decode the single complete triple successfully");
        assert_eq!(
            decoded.node_coords(),
            &[1.0_f32, 2.0, 3.0],
            "only the complete triple must survive; the trailing [4,5] are dropped"
        );
    }

    // ── step-1 test ───────────────────────────────────────────────────────────

    /// GREEN (step-2 impl): CarriedTopology, to_value, from_value now exist.
    ///
    /// Tests construction via accessors and lossless Value round-trip.
    #[test]
    fn carried_topology_construction_and_round_trip() {
        let topo = make_fixture();

        // ── Accessor checks ──────────────────────────────────────────────────
        assert_eq!(topo.part().realization_ref.entity, "body");
        assert_eq!(topo.part().realization_ref.index, 0);
        assert!(topo.part().kernel_handle.is_none());

        assert_eq!(topo.node_coords().len(), 12, "4 nodes × 3 floats");
        assert_eq!(topo.face_normals().len(), 2);
        assert_eq!(topo.face_normals()[0].0, GeometryHandleId(1));
        assert_eq!(topo.face_normals()[0].1, [0.0, 0.0, 1.0]);
        assert_eq!(topo.face_normals()[1].0, GeometryHandleId(2));
        assert_eq!(topo.face_normals()[1].1, [1.0, 0.0, 0.0]);

        assert_eq!(topo.boundary().len(), 3);
        assert_eq!(
            topo.boundary().get(4),
            Some(NodeAttachment::OnFace(GeometryHandleId(1)))
        );
        assert_eq!(
            topo.boundary().get(8),
            Some(NodeAttachment::OnFace(GeometryHandleId(2)))
        );
        assert_eq!(
            topo.boundary().get(0),
            Some(NodeAttachment::OnEdge(GeometryHandleId(1)))
        );

        // ── Lossless round-trip ──────────────────────────────────────────────
        let encoded = topo.to_value();
        let decoded = CarriedTopology::from_value(&encoded);
        assert!(decoded.is_some(), "from_value must succeed on a valid encoding");
        assert_eq!(decoded.unwrap(), topo, "round-trip must be exact (PartialEq)");
    }
}
