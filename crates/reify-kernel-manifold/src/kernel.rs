//! `ManifoldKernel` — Manifold mesh-Boolean kernel adapter.
//!
//! Manifold C++ FFI is wired via `manifold3d` 0.1 (the
//! `zmerlynn/manifold-csg` fork). The kernel maintains a per-handle
//! `HashMap<u64, manifold3d::Manifold>` store mirroring `OcctKernel`'s
//! storage pattern (`crates/reify-kernel-occt/src/lib.rs:456-466`).
//!
//! # Design templates
//!
//! `crates/reify-kernel-occt/src/lib.rs` — storage pattern (HashMap of
//! per-handle native shapes, `next_id` counter, `store/get_*` helpers).
//! `crates/reify-test-support/src/mocks.rs:889` — `FailingMockGeometryKernel`.
//!
//! # KernelAttributeHook impl (PRD line 70)
//!
//! ManifoldKernel is the first concrete impl of
//! [`reify_types::KernelAttributeHook`] — see PRD
//! `docs/prds/v0_2/persistent-naming-v2.md` line 70 ("Multi-kernel
//! propagation via `KernelAttributeHook` trait"). The
//! [`GeometryKernel::attribute_hook`] override on `ManifoldKernel` returns
//! `Some(self)` so the engine-side dispatcher
//! (`reify_eval::propagate_via_kernel_attribute_hook`) routes Manifold ops
//! through the hook.
//!
//! ## Task-9-pending stub semantics
//!
//! [`KernelAttributeHook::propagate_attributes`] currently returns
//! `Ok(KernelAttributeOutcome::Discarded)` and emits a
//! `tracing::warn!(reason="task_9_pending", …)` event before returning.
//! The Manifold C++ FFI is wired and the manifold3d accessors
//! (`originalID`, `MeshGL.run_*`, merge vectors, etc.) are reachable from
//! this crate, but the actual MeshGL walk is implemented in
//! persistent-naming-v2 PRD task 9 (a separate task that depends on this
//! crate's FFI wiring). The trait surface is stable across that swap; only
//! the body changes.

use std::collections::HashMap;

use manifold3d::Manifold;
use reify_ir::{ExportError, ExportFormat, FeatureId, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, KernelAttributeHook, KernelAttributeOutcome, Mesh, QueryError, TessError, TopologyAttributeTable, Value};

/// Error message used by the v0.2 stub paths (`query`/`export`) that
/// have not yet been wired to real FFI. Boolean ops (`Union`,
/// `Difference`, `Intersection`) and `tessellate` are now wired via
/// `manifold3d` 0.1; `query`/`export` remain follow-up work for v0.2.
const STUB_MSG: &str = "Manifold query/export not yet implemented for v0.2; \
    boolean ops and tessellate are wired via manifold3d 0.1, but query/export \
    are follow-up work (see docs/prds/v0_2/multi-kernel.md).";

/// A sub-element (face triangle or edge segment) extracted from a parent
/// Manifold mesh by [`GeometryKernel::extract_faces`] /
/// [`GeometryKernel::extract_edges`].
///
/// A single triangle or edge is **not** a closed [`Manifold`] and cannot live
/// in the [`ManifoldKernel::shapes`] store, so extracted sub-elements are
/// persisted in a parallel typed store ([`ManifoldKernel::sub_shapes`]) keyed
/// by the same id space. `query()` distinguishes a sub-handle from a full-mesh
/// handle by store membership: an id present in `sub_shapes` answers
/// per-element property queries (`SurfaceArea`, `FaceNormal`, `EdgeTangent`,
/// `BoundingBox`); an id present in `shapes` answers whole-mesh queries.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SubShape {
    /// A mesh triangle: three xyz corner points in winding order.
    Face([[f64; 3]; 3]),
    /// A mesh edge: two xyz endpoints.
    Edge([[f64; 3]; 2]),
}

/// Manifold mesh-Boolean kernel adapter, backed by `manifold3d` 0.1.
///
/// Mirrors `OcctKernel`'s storage shape (`crates/reify-kernel-occt/src/lib.rs:456-466`):
/// per-handle native shapes in a `HashMap<u64, _>` with a monotonic
/// `next_id` counter. Manifold's [`Manifold`] is `Send + Sync` (per the
/// `unsafe impl` blocks in `manifold-csg`'s `manifold.rs`), so
/// `ManifoldKernel` auto-derives `Send + Sync` without needing an
/// actor-thread analogue of `OcctKernelHandle`.
pub struct ManifoldKernel {
    /// Per-handle stored Manifolds. Inserted by [`Self::store`] (called from
    /// `execute` boolean arms and from the `test-fixtures` ingestion path);
    /// looked up by `tessellate` and the boolean arms.
    shapes: HashMap<u64, Manifold>,
    /// Per-handle extracted sub-elements (face triangles / edge segments).
    /// Inserted by [`Self::store_sub_shape`] (called from `extract_faces` /
    /// `extract_edges`); looked up by the per-element `query()` arms. Keyed
    /// in the same id space as [`Self::shapes`] (both mint from `next_id`),
    /// so a sub-handle never aliases a full-mesh handle.
    sub_shapes: HashMap<u64, SubShape>,
    /// Monotonic id counter; first allocated handle is `1` (matches OCCT).
    /// `0` and `u64::MAX` are reserved (the latter is `GeometryHandleId::INVALID`).
    next_id: u64,
}

impl ManifoldKernel {
    /// Construct a new `ManifoldKernel` with empty storage.
    pub fn new() -> Self {
        Self {
            shapes: HashMap::new(),
            sub_shapes: HashMap::new(),
            next_id: 1,
        }
    }

    /// Store a `Manifold` and return its newly-allocated handle.
    ///
    /// `repr` is `None`: Manifold's `Manifold` belongs to the
    /// [`ReprKind::Mesh`] family — there is no meaningful B-rep sub-shape
    /// classification for a mesh kernel, so `repr` carries `None` per task
    /// 3179's architectural decision (option (b)). See also task 3093 review
    /// esc-3093-33, which first identified the semantic abuse.
    fn store(&mut self, manifold: Manifold) -> GeometryHandle {
        let id = self.next_id;
        self.next_id += 1;
        self.shapes.insert(id, manifold);
        GeometryHandle {
            id: GeometryHandleId(id),
            repr: None,
        }
    }

    /// Look up a stored [`Manifold`] by handle, returning
    /// [`GeometryError::InvalidReference`] when the id is not present.
    ///
    /// Mirrors `OcctKernel::get_shape` (`crates/reify-kernel-occt/src/lib.rs:516-523`).
    /// Centralising the lookup in one helper keeps the InvalidReference
    /// surface uniform across `execute`'s boolean arms — `tessellate`
    /// surfaces the same shape via [`TessError::InvalidHandle`] (the
    /// per-trait variant; `GeometryError` and `TessError` are sibling
    /// error enums).
    fn get_manifold(&self, id: GeometryHandleId) -> Result<&Manifold, GeometryError> {
        self.shapes
            .get(&id.0)
            .ok_or(GeometryError::InvalidReference(id))
    }

    /// Store an extracted [`SubShape`] (face triangle / edge segment) under a
    /// fresh handle id minted from the shared `next_id` counter, and return
    /// that id.
    ///
    /// Sharing `next_id` with [`Self::store`] keeps sub-handle ids globally
    /// unique so a sub-handle never aliases a full-mesh handle — `query()`
    /// can therefore route by store membership (`sub_shapes` vs `shapes`)
    /// without ambiguity.
    fn store_sub_shape(&mut self, sub: SubShape) -> GeometryHandleId {
        let id = self.next_id;
        self.next_id += 1;
        self.sub_shapes.insert(id, sub);
        GeometryHandleId(id)
    }
}

impl Default for ManifoldKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl GeometryKernel for ManifoldKernel {
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        match op {
            GeometryOp::Union { left, right } => {
                let l = self.get_manifold(*left)?;
                let r = self.get_manifold(*right)?;
                let result = l.union(r);
                Ok(self.store(result))
            }
            GeometryOp::Difference { left, right } => {
                let l = self.get_manifold(*left)?;
                let r = self.get_manifold(*right)?;
                let result = l.difference(r);
                Ok(self.store(result))
            }
            GeometryOp::Intersection { left, right } => {
                let l = self.get_manifold(*left)?;
                let r = self.get_manifold(*right)?;
                let result = l.intersection(r);
                Ok(self.store(result))
            }
            // Non-boolean ops are out of scope for the v0.2 manifold
            // adapter — see `STUB_MSG`.
            _ => Err(GeometryError::OperationFailed(STUB_MSG.into())),
        }
    }

    fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        match query {
            // Distance between two manifold meshes — exact surface-to-surface
            // via Manifold::min_gap (manifold3d 0.2).  Returns 0.0 for
            // touching/interpenetrating; returns the true gap for disjoint solids.
            // PRD §9 KGQ-α / task 3610; generalised to min_gap by KGQ-ο / task 3624.
            GeometryQuery::Distance { from, to } => {
                let a = self
                    .get_manifold(*from)
                    .map_err(|e| QueryError::QueryFailed(format!("{e:?}")))?;
                let b = self
                    .get_manifold(*to)
                    .map_err(|e| QueryError::QueryFailed(format!("{e:?}")))?;
                let d = crate::queries::distance(a, b);
                // queries::distance returns f64::INFINITY when one or both
                // meshes have no usable vertices (extract_xyz is empty).
                // Propagating an infinite length would be silently wrong —
                // the invariant-#3 contract requires visible degradation, so
                // we convert the sentinel to a QueryError here and let the
                // kernel_distance helper emit exactly one Warning diagnostic
                // (reviewer suggestion on empty-mesh robustness).
                if d.is_infinite() {
                    return Err(QueryError::QueryFailed(
                        "distance: one or both meshes have no usable vertices \
                         (degenerate or empty manifold)"
                            .into(),
                    ));
                }
                Ok(Value::Real(d))
            }
            // Point-in-solid via ray-cast crossing count.
            // PRD §5.4 KGQ-β / task 3624 (KGQ-ο).
            GeometryQuery::Contains {
                handle,
                px,
                py,
                pz,
                tolerance,
            } => {
                let m = self
                    .get_manifold(*handle)
                    .map_err(|e| QueryError::QueryFailed(format!("{e:?}")))?;
                Ok(Value::Bool(crate::queries::contains(m, *px, *py, *pz, *tolerance)))
            }
            // Topology-signature + sampled-vertex geometric equivalence check.
            // PRD §5.1 KGQ-δ / task 3624 (KGQ-ο).
            GeometryQuery::GeoEquiv {
                left,
                right,
                tolerance,
            } => {
                let l = self
                    .get_manifold(*left)
                    .map_err(|e| QueryError::QueryFailed(format!("{e:?}")))?;
                let r = self
                    .get_manifold(*right)
                    .map_err(|e| QueryError::QueryFailed(format!("{e:?}")))?;
                Ok(Value::Bool(crate::queries::geo_equiv(l, r, *tolerance)))
            }
            // Surface area. Mirrors OCCT's SurfaceArea -> Value::Real
            // (KGQ-π / task 3625). A face sub-handle answers with its single
            // triangle's area; a whole-mesh handle answers with the
            // Manifold's total surface area.
            GeometryQuery::SurfaceArea(id) => {
                if let Some(sub) = self.sub_shapes.get(&id.0) {
                    match sub {
                        SubShape::Face(tri) => Ok(Value::Real(crate::queries::tri_area(tri))),
                        SubShape::Edge(_) => Err(QueryError::QueryFailed(
                            "SurfaceArea: handle names an edge sub-shape, which has no area"
                                .into(),
                        )),
                    }
                } else if let Some(m) = self.shapes.get(&id.0) {
                    Ok(Value::Real(m.surface_area()))
                } else {
                    Err(QueryError::InvalidHandle(*id))
                }
            }
            // Face normal as the OCCT-compatible {"x","y","z"} JSON string.
            // Only a face sub-handle has a single normal; a whole mesh or an
            // edge sub-shape has none (matches OCCT, which answers FaceNormal
            // only for a Face). Sign follows triangle winding — the contract
            // is sign-agnostic.
            GeometryQuery::FaceNormal(id) => match self.sub_shapes.get(&id.0) {
                Some(SubShape::Face(tri)) => Ok(Value::String(crate::queries::json_xyz(
                    crate::queries::tri_unit_normal(tri),
                ))),
                Some(SubShape::Edge(_)) => Err(QueryError::QueryFailed(
                    "FaceNormal: handle names an edge sub-shape (no face normal)".into(),
                )),
                None => {
                    if self.shapes.contains_key(&id.0) {
                        Err(QueryError::QueryFailed(
                            "FaceNormal: handle names a whole mesh, which has no single face \
                             normal; query an extracted face sub-handle instead"
                                .into(),
                        ))
                    } else {
                        Err(QueryError::InvalidHandle(*id))
                    }
                }
            },
            // Edge tangent as the OCCT-compatible {"x","y","z"} JSON string.
            // Only an edge sub-handle has a tangent; a whole mesh or a face
            // sub-shape has none. Sign follows the stored endpoint order — the
            // contract is sign-agnostic.
            GeometryQuery::EdgeTangent(id) => match self.sub_shapes.get(&id.0) {
                Some(SubShape::Edge(edge)) => Ok(Value::String(crate::queries::json_xyz(
                    crate::queries::edge_unit_tangent(edge),
                ))),
                Some(SubShape::Face(_)) => Err(QueryError::QueryFailed(
                    "EdgeTangent: handle names a face sub-shape (no edge tangent)".into(),
                )),
                None => {
                    if self.shapes.contains_key(&id.0) {
                        Err(QueryError::QueryFailed(
                            "EdgeTangent: handle names a whole mesh, not an edge; query an \
                             extracted edge sub-handle instead"
                                .into(),
                        ))
                    } else {
                        Err(QueryError::InvalidHandle(*id))
                    }
                }
            },
            // Bounding box as the OCCT-compatible {"xmin"..."zmax"} JSON
            // string. A sub-shape (face/edge) bounds its stored points; a
            // whole mesh delegates to Manifold::bounding_box() (None =>
            // empty/degenerate => QueryError).
            GeometryQuery::BoundingBox(id) => {
                if let Some(sub) = self.sub_shapes.get(&id.0) {
                    let (min, max) = match sub {
                        SubShape::Face(tri) => crate::queries::points_bbox(tri),
                        SubShape::Edge(edge) => crate::queries::points_bbox(edge),
                    };
                    Ok(Value::String(crate::queries::json_bbox(min, max)))
                } else if let Some(m) = self.shapes.get(&id.0) {
                    match m.bounding_box() {
                        Some(bb) => {
                            Ok(Value::String(crate::queries::json_bbox(bb.min(), bb.max())))
                        }
                        None => Err(QueryError::QueryFailed(
                            "BoundingBox: empty/degenerate manifold has no bounding box".into(),
                        )),
                    }
                } else {
                    Err(QueryError::InvalidHandle(*id))
                }
            }
            // Faces (mesh triangles) sharing at least one edge with triangle
            // `face_index`, self excluded, ascending — Value::List<Value::Int>
            // mirroring OCCT's AdjacentFaces wire format. On the closed cube
            // each triangle has exactly 3 such neighbours. (KGQ-π / task 3625.)
            GeometryQuery::AdjacentFaces { shape, face_index } => {
                let (_verts, tris) = {
                    let m = self
                        .get_manifold(*shape)
                        .map_err(|e| QueryError::QueryFailed(format!("{e:?}")))?;
                    crate::queries::mesh_geometry(m)
                };
                let triangles = crate::queries::triangles_of(&tris);
                match crate::queries::adjacent_faces(&triangles, *face_index) {
                    Some(neighbours) => Ok(Value::List(
                        neighbours.into_iter().map(|i| Value::Int(i as i64)).collect(),
                    )),
                    None => Err(QueryError::QueryFailed(format!(
                        "AdjacentFaces: face_index {} out of range 0..{}",
                        face_index,
                        triangles.len()
                    ))),
                }
            }
            // Canonical edge indices shared by triangles `face_a` and `face_b`,
            // ascending — Value::List<Value::Int> mirroring OCCT. `face_a ==
            // face_b` yields an empty list (design decision). Edge indices are
            // into the same canonical_edges enumeration extract_edges exposes,
            // so SharedEdges and extract_edges agree. (KGQ-π / task 3625.)
            GeometryQuery::SharedEdges {
                shape,
                face_a,
                face_b,
            } => {
                let (verts, tris) = {
                    let m = self
                        .get_manifold(*shape)
                        .map_err(|e| QueryError::QueryFailed(format!("{e:?}")))?;
                    crate::queries::mesh_geometry(m)
                };
                let triangles = crate::queries::triangles_of(&tris);
                let (index_pairs, _endpoints) = crate::queries::canonical_edges(&verts, &tris);
                match crate::queries::shared_edges(&triangles, &index_pairs, *face_a, *face_b) {
                    Some(shared) => Ok(Value::List(
                        shared.into_iter().map(|i| Value::Int(i as i64)).collect(),
                    )),
                    None => Err(QueryError::QueryFailed(format!(
                        "SharedEdges: face index out of range 0..{} (face_a={}, face_b={})",
                        triangles.len(),
                        face_a,
                        face_b
                    ))),
                }
            }
            // Center of mass via signed-tetrahedron mesh integration. `density`
            // is intentionally ignored (bound to `_`): for a uniform-density
            // solid the centre of mass IS the geometric volume centroid, so the
            // result matches OCCT's density-ignoring CenterOfMass exactly.
            // Value::String {"x","y","z"} (OCCT wire format); empty/degenerate
            // mesh (V≈0) => QueryFailed. (KGQ-π / task 3625.)
            GeometryQuery::CenterOfMass { handle, density: _ } => {
                let (verts, tris) = {
                    let m = self
                        .get_manifold(*handle)
                        .map_err(|e| QueryError::QueryFailed(format!("{e:?}")))?;
                    crate::queries::mesh_geometry(m)
                };
                match crate::queries::mass_properties(&verts, &tris) {
                    Some(mp) => Ok(Value::String(crate::queries::json_xyz(mp.centroid))),
                    None => Err(QueryError::QueryFailed(
                        "CenterOfMass: empty/degenerate mesh has no centroid".into(),
                    )),
                }
            }
            // All other queries remain follow-up work (see STUB_MSG).
            _ => Err(QueryError::QueryFailed(STUB_MSG.into())),
        }
    }

    fn export(
        &self,
        _handle: GeometryHandleId,
        _format: ExportFormat,
        _writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        Err(ExportError::FormatError(STUB_MSG.into()))
    }

    /// Materialise the stored [`Manifold`] as a `reify_types::Mesh`.
    ///
    /// `tolerance` is intentionally unused at this layer — manifold meshes
    /// are exact, and the underlying [`Manifold`] carries its own tolerance
    /// set at construction (see `manifold-csg`'s tolerance-tracking
    /// invariants). Callers passing non-zero values are not rejected; the
    /// argument is accepted for trait-conformance with [`GeometryKernel`].
    ///
    /// f64→f32 narrowing happens at this boundary because Reify's
    /// `Mesh.vertices: Vec<f32>` is the boundary contract (per Decision 4
    /// in the task plan: "narrow at the boundary; manifold internals stay
    /// f64"). `n_props` from `to_mesh_f64` is `3` (xyz) for the position-
    /// only meshes this kernel ingests; we extract only the first three
    /// properties per vertex to stay robust against manifold internally
    /// growing the property block (e.g. merge-tag layers).
    fn tessellate(&self, handle: GeometryHandleId, _tolerance: f64) -> Result<Mesh, TessError> {
        let manifold = self
            .shapes
            .get(&handle.0)
            .ok_or(TessError::InvalidHandle(handle))?;

        let (vert_props_f64, n_props, tri_indices_u64) = manifold.to_mesh_f64();

        // Empty/degenerate-manifold short-circuit. A boolean op that
        // produces no overlap (e.g. `Intersection` of disjoint cubes) can
        // surface as `n_props == 0` or empty `vert_props_f64`; without
        // this guard, `vert_props_f64.len() / n_props` panics with
        // divide-by-zero in release builds. Returning an empty `Mesh` is
        // the structurally honest answer — callers can detect it via
        // `mesh.vertices.is_empty()`.
        if n_props == 0 || vert_props_f64.is_empty() {
            return Ok(Mesh {
                vertices: Vec::new(),
                indices: Vec::new(),
                normals: None,
            });
        }

        // For valid (non-empty) manifolds, manifold3d guarantees at least
        // xyz; surface a runtime `TessError` rather than panicking on a
        // corrupted result so callers can recover.
        if n_props < 3 {
            return Err(TessError::TessellationFailed(format!(
                "manifold3d::to_mesh_f64 returned n_props={n_props}; \
                 need at least 3 (xyz) for a Reify Mesh",
            )));
        }

        // Extract xyz triplets from each n_props-sized vertex block.
        // For our position-only meshes n_props == 3, but manifold may
        // internally maintain additional property layers; we deliberately
        // copy only the first three.
        let n_verts = vert_props_f64.len() / n_props;
        let mut vertices: Vec<f32> = Vec::with_capacity(n_verts * 3);
        for v in 0..n_verts {
            let base = v * n_props;
            vertices.push(vert_props_f64[base] as f32);
            vertices.push(vert_props_f64[base + 1] as f32);
            vertices.push(vert_props_f64[base + 2] as f32);
        }

        // u64→u32 narrowing: manifold's u64 indices are nominal; in
        // practice meshes that fit Reify's Vec<u32> contract have
        // <= 4-billion vertices. We use `u32::try_from` rather than
        // `as u32` so a corrupted Manifold (or future contract change)
        // surfaces as an observable `TessError::TessellationFailed`
        // rather than silently truncating to a structurally invalid
        // Mesh whose downstream consumers would index out-of-bounds.
        let indices: Vec<u32> = tri_indices_u64
            .iter()
            .map(|&i| {
                u32::try_from(i).map_err(|_| {
                    TessError::TessellationFailed(format!(
                        "manifold3d returned triangle index {i} > u32::MAX; \
                         Reify Mesh.indices is Vec<u32>",
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Mesh {
            vertices,
            indices,
            normals: None,
        })
    }
    /// Extract the mesh triangles of the stored Manifold as face sub-handles.
    ///
    /// # Manifold-face = mesh triangle (semantic gap)
    ///
    /// Unlike a B-rep kernel (where a "face" is a smooth parametric surface
    /// patch), `manifold-csg` has no coalesced-surface concept — only mesh
    /// facets. So this returns **one sub-handle per triangle**: the unit cube
    /// yields 12 face handles, not the 6 a BRep box reports. See the
    /// `queries` module-doc and PRD Open Question §10.5; `AdjacentFaces` /
    /// `SharedEdges` therefore operate on triangle indices.
    ///
    /// Each triangle's three xyz corners (in mesh winding order) are stored as
    /// a [`SubShape::Face`] via [`Self::store_sub_shape`]; the returned
    /// `Vec<GeometryHandleId>` is in triangle order, so `result[i]` names
    /// triangle `i` of `to_mesh_f64`'s index list. An empty or degenerate mesh
    /// (e.g. the empty `Manifold` from a disjoint intersection) yields
    /// `Ok(empty vec)`.
    fn extract_faces(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        // Read the parent mesh, dropping the immutable borrow before the
        // mutable store_sub_shape calls below.
        let (verts, tris) = {
            let m = self
                .get_manifold(handle)
                .map_err(|e| QueryError::QueryFailed(format!("{e:?}")))?;
            crate::queries::mesh_geometry(m)
        };
        if verts.is_empty() || tris.is_empty() {
            return Ok(Vec::new());
        }
        let mut faces = Vec::with_capacity(tris.len() / 3);
        for tri in tris.chunks_exact(3) {
            let v0 = verts[tri[0] as usize];
            let v1 = verts[tri[1] as usize];
            let v2 = verts[tri[2] as usize];
            faces.push(self.store_sub_shape(SubShape::Face([v0, v1, v2])));
        }
        Ok(faces)
    }

    /// Extract the unique undirected mesh edges of the stored Manifold as
    /// edge sub-handles.
    ///
    /// Uses the canonical edge enumeration ([`crate::queries::canonical_edges`])
    /// — deduped undirected vertex-index pairs, ordered ascending by
    /// `(min_idx, max_idx)` — so the returned `Vec<GeometryHandleId>` is in
    /// canonical edge order: `result[e]` names canonical edge `e`, the same
    /// index space `SharedEdges` reports. The unit cube has 18 such edges
    /// (Euler `V - E + F = 2`: `8 - E + 12 = 2`), matching
    /// `Manifold::num_edge()`. Each edge's two xyz endpoints are stored as a
    /// [`SubShape::Edge`]. An empty/degenerate mesh yields `Ok(empty vec)`.
    fn extract_edges(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        // Read the parent mesh, dropping the immutable borrow before the
        // mutable store_sub_shape calls below.
        let (verts, tris) = {
            let m = self
                .get_manifold(handle)
                .map_err(|e| QueryError::QueryFailed(format!("{e:?}")))?;
            crate::queries::mesh_geometry(m)
        };
        if verts.is_empty() || tris.is_empty() {
            return Ok(Vec::new());
        }
        let (_index_pairs, endpoints) = crate::queries::canonical_edges(&verts, &tris);
        let mut edges = Vec::with_capacity(endpoints.len());
        for ep in endpoints {
            edges.push(self.store_sub_shape(SubShape::Edge(ep)));
        }
        Ok(edges)
    }

    // extract_vertices, execute_with_history, and query_many use the trait
    // defaults — they error in the standard "not supported" fashion.

    /// Ingest an externally-supplied [`Mesh`] into the kernel, converting it
    /// to a `Manifold` and storing it under a fresh handle.
    ///
    /// # Widening rationale (Decision 4, task 3186 plan)
    ///
    /// Reify's boundary contract is `Mesh { vertices: Vec<f32>, indices:
    /// Vec<u32> }` while `Manifold::from_mesh_f64` requires `f64` vertex
    /// props and `u64` indices. The widening (`f32 as f64`, `u32 as u64`)
    /// happens here at the ingestion seam; manifold internals remain f64
    /// throughout, and the corresponding narrowing on egress (`tessellate`)
    /// converts back to f32/u32 at the Reify boundary.
    ///
    /// # Error surface
    ///
    /// Returns `Err(GeometryError::OperationFailed(_))` if the input is not a
    /// closed orientable manifold (e.g. a mesh with boundary edges, inverted
    /// winding, or degenerate geometry). The underlying `manifold3d` error is
    /// included in the `OperationFailed` payload so winding-order regressions
    /// in fixture meshes are debuggable without source-diving.
    fn ingest_mesh(&mut self, mesh: &Mesh) -> Result<GeometryHandle, GeometryError> {
        if !mesh.vertices.len().is_multiple_of(3) {
            return Err(GeometryError::OperationFailed(format!(
                "ingest_mesh: vertices.len() must be a multiple of 3 (xyz triplets); \
                 got {}",
                mesh.vertices.len()
            )));
        }
        if !mesh.indices.len().is_multiple_of(3) {
            return Err(GeometryError::OperationFailed(format!(
                "ingest_mesh: indices.len() must be a multiple of 3 (triangle triplets); \
                 got {}",
                mesh.indices.len()
            )));
        }
        let vert_props_f64: Vec<f64> = mesh.vertices.iter().map(|&v| v as f64).collect();
        let tri_indices_u64: Vec<u64> = mesh.indices.iter().map(|&i| i as u64).collect();
        let manifold =
            Manifold::from_mesh_f64(&vert_props_f64, 3, &tri_indices_u64).map_err(|e| {
                GeometryError::OperationFailed(format!(
                    "ingest_mesh: input Mesh must be a valid manifold; \
                     manifold3d::from_mesh_f64 reported: {e:?}"
                ))
            })?;
        Ok(self.store(manifold))
    }

    /// Override the trait default to advertise that ManifoldKernel implements
    /// [`KernelAttributeHook`]. Per PRD line 70, ManifoldKernel is the first
    /// concrete impl: returning `Some(self)` here is what makes the engine-
    /// side dispatcher (`reify-eval::propagate_via_kernel_attribute_hook`)
    /// route attribute propagation to [`Self::propagate_attributes`] rather
    /// than `KernelAttributeOutcome::FellThrough`.
    fn attribute_hook(&self) -> Option<&dyn KernelAttributeHook> {
        Some(self)
    }
}

/// First concrete impl of [`KernelAttributeHook`] — see PRD line 70.
///
/// The body unconditionally returns `Ok(KernelAttributeOutcome::Discarded)`
/// and emits a structured WARN diagnostic (required by the `Discarded`
/// contract). The Manifold C++ FFI is wired (boolean ops + tessellate go
/// through `manifold3d` 0.1) and the manifold3d accessors needed for real
/// propagation (`originalID`, `MeshGL.run_*`, `merge_from_vert`/
/// `merge_to_vert`, `face_id`) are reachable from this crate; the actual
/// `MeshGL` walk is implemented in persistent-naming-v2 PRD task 9 (a
/// separate task that depends on this crate's FFI wiring).
///
/// When PRD task 9 lands, the body switches to walk `MeshGL` merge
/// vectors + per-triangle `faceID` / `originalID` to copy parent
/// attributes onto result face handles, returning `Propagated` on success
/// and `Discarded` (with a `reason="heavy_remeshing"` flavoured WARN) on
/// lossy remeshing — the trait surface is stable across that swap.
impl KernelAttributeHook for ManifoldKernel {
    fn propagate_attributes(
        &self,
        _table: &mut TopologyAttributeTable,
        op: &GeometryOp,
        parent_handles: &[GeometryHandleId],
        _result_handle: GeometryHandleId,
        _splitting_feature_id: &FeatureId,
    ) -> Result<KernelAttributeOutcome, QueryError> {
        // v0.2 stub: FFI is wired but the MeshGL walk that implements
        // real attribute propagation is PRD task 9 (persistent-naming-v2).
        // Emit a WARN diagnostic (operator visibility for the intentional
        // attribute-loss path) and return Discarded. The
        // `KernelAttributeOutcome::Discarded` contract mandates that hook
        // impls emit their own diagnostic before returning, so consumers
        // do not need to surface a duplicate.
        //
        // `target: "reify_kernel_manifold::kernel"` matches the module
        // path of this impl so a `RUST_LOG=reify_kernel_manifold::kernel=warn`
        // (or the broader `reify_kernel_manifold=warn`) operator filter
        // sees the event. `reason="task_9_pending"` is the structured-
        // fields key by which a future `reason="heavy_remeshing"` (when
        // PRD task 9 lands the real walk) can be distinguished.
        tracing::warn!(
            target: "reify_kernel_manifold::kernel",
            reason = "task_9_pending",
            op = ?op,
            parents = parent_handles.len(),
            "Manifold attribute propagation discarded — MeshGL walk pending (PRD task 9)"
        );
        Ok(KernelAttributeOutcome::Discarded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the keepable structural property that the macro
    /// `reify_test_support::assert_stub_kernel_errors!` was previously
    /// generating: `ManifoldKernel` is `Send + Sync` and round-trips through a
    /// `Box<dyn GeometryKernel>` upcast. The macro's other generated tests
    /// (which pinned "every method returns Err with substring 'Manifold'") are
    /// intentionally NOT preserved here — they directly contradict the
    /// post-FFI contract where Union/Difference/Intersection succeed on valid
    /// handles.
    #[test]
    fn manifold_kernel_implements_geometry_kernel_trait() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ManifoldKernel>();
        let _boxed: Box<dyn reify_ir::GeometryKernel> = Box::new(ManifoldKernel::new());
    }

    // The `unit_cube_mesh` helper used by the boolean-op tests below
    // lives in [`crate::test_fixtures`] so the same fixture is shared by
    // the cross-crate integration tests under `tests/` (avoids drift).
    #[cfg(feature = "test-fixtures")]
    use crate::test_fixtures::unit_cube_mesh;

    /// Pin macro-helper: structural `Ok(GeometryHandle)` shape for the three
    /// boolean op tests below. Match-on-Ok rather than `assert_eq!` because
    /// `GeometryError` does not derive `PartialEq`.
    #[cfg(feature = "test-fixtures")]
    fn assert_ok_handle(result: Result<GeometryHandle, GeometryError>, label: &str) {
        match result {
            Ok(GeometryHandle { id, .. }) => {
                assert_ne!(
                    id,
                    GeometryHandleId::INVALID,
                    "{label} must return a real (non-INVALID) handle id",
                );
            }
            other => panic!(
                "{label} of two valid stored cubes must return Ok(GeometryHandle); got {other:?}"
            ),
        }
    }

    /// Pins that `execute(GeometryOp::Union)` over two stored unit cubes
    /// returns `Ok(GeometryHandle { .. })`.
    ///
    /// Match-on-Ok-with-id rather than `assert_eq!` because `GeometryError`
    /// does not derive `PartialEq`. The `repr: None` contract is pinned
    /// separately by `manifold_kernel_handle_repr_is_none_for_non_brep_kernel`;
    /// this test only pins the structural `Ok(GeometryHandle { .. })` shape.
    #[cfg(feature = "test-fixtures")]
    #[test]
    fn union_of_two_stored_cubes_returns_ok_handle() {
        let mut kernel = ManifoldKernel::new();
        let l = kernel
            .ingest_mesh(&unit_cube_mesh([0.0, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold")
            .id;
        let r = kernel
            .ingest_mesh(&unit_cube_mesh([0.5, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold")
            .id;

        let result = kernel.execute(&GeometryOp::Union { left: l, right: r });

        assert_ok_handle(result, "Union");
    }

    /// Pins that `execute(GeometryOp::Difference)` over two overlapping
    /// stored unit cubes returns `Ok(GeometryHandle { .. })`.
    ///
    /// Cubes overlap by 0.5 in x so the difference is a non-degenerate
    /// volume (no early empty-result short-circuit).
    #[cfg(feature = "test-fixtures")]
    #[test]
    fn difference_of_two_stored_cubes_returns_ok_handle() {
        let mut kernel = ManifoldKernel::new();
        let l = kernel
            .ingest_mesh(&unit_cube_mesh([0.0, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold")
            .id;
        let r = kernel
            .ingest_mesh(&unit_cube_mesh([0.5, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold")
            .id;

        let result = kernel.execute(&GeometryOp::Difference { left: l, right: r });

        assert_ok_handle(result, "Difference");
    }

    /// Pins that `execute(GeometryOp::Intersection)` over two overlapping
    /// stored unit cubes returns `Ok(GeometryHandle { .. })`.
    ///
    /// Cubes overlap by 0.5 in x so the intersection has non-empty volume.
    /// We deliberately do NOT pin the geometric volume here (that's a
    /// query, exercised separately) — only the structural handle-return
    /// contract. The disjoint-input empty-mesh contract is exercised
    /// separately by
    /// [`tessellate_of_intersection_of_disjoint_cubes_returns_empty_mesh`].
    #[cfg(feature = "test-fixtures")]
    #[test]
    fn intersection_of_two_overlapping_cubes_returns_ok_handle() {
        let mut kernel = ManifoldKernel::new();
        let l = kernel
            .ingest_mesh(&unit_cube_mesh([0.0, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold")
            .id;
        let r = kernel
            .ingest_mesh(&unit_cube_mesh([0.5, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold")
            .id;

        let result = kernel.execute(&GeometryOp::Intersection { left: l, right: r });

        assert_ok_handle(result, "Intersection");
    }

    /// Pins the empty-/degenerate-manifold short-circuit in
    /// [`ManifoldKernel::tessellate`] (kernel.rs `n_props == 0 ||
    /// vert_props_f64.is_empty()` branch).
    ///
    /// Two cubes offset 5 units in x cannot overlap, so
    /// `Manifold::intersection` returns an empty Manifold. Without the
    /// short-circuit, `tessellate` would panic with a divide-by-zero in
    /// release builds when computing `vert_props_f64.len() / n_props`. The
    /// structurally honest answer is an empty `Mesh` (no vertices, no
    /// indices) — callers detect it via `mesh.vertices.is_empty()`.
    ///
    /// Added during amendment round 2 (was previously uncovered: a
    /// regression that removed the short-circuit would only surface as a
    /// release-build panic on disjoint-input boolean callers).
    #[cfg(feature = "test-fixtures")]
    #[test]
    fn tessellate_of_intersection_of_disjoint_cubes_returns_empty_mesh() {
        let mut kernel = ManifoldKernel::new();
        let l = kernel
            .ingest_mesh(&unit_cube_mesh([0.0, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold")
            .id;
        // Offset >> 1.0 so the two cubes share no volume.
        let r = kernel
            .ingest_mesh(&unit_cube_mesh([5.0, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold")
            .id;

        let intersection_handle = kernel
            .execute(&GeometryOp::Intersection { left: l, right: r })
            .expect("Intersection of two valid (disjoint) cubes must Ok-return a handle");

        let mesh = kernel.tessellate(intersection_handle.id, 0.0).expect(
            "tessellate of empty/degenerate Manifold must Ok-return an empty Mesh, \
                 not panic via the divide-by-zero short-circuit guard",
        );

        assert!(
            mesh.vertices.is_empty(),
            "tessellated empty intersection must have zero vertices; got {} f32s",
            mesh.vertices.len(),
        );
        assert!(
            mesh.indices.is_empty(),
            "tessellated empty intersection must have zero indices; got {} u32s",
            mesh.indices.len(),
        );
    }

    /// RED for step-9 of task 3093: pins that `execute(GeometryOp::Union
    /// { left, right })` with handles unknown to the kernel returns
    /// `Err(GeometryError::InvalidReference(_))`.
    ///
    /// Currently fails because the Union arm propagates a generic
    /// `OperationFailed("…not found")` (per the placeholder in step-2).
    /// Step-10 introduces a centralised `get_manifold` helper that returns
    /// `InvalidReference(id)` and wires all three boolean arms +
    /// `tessellate` to use it.
    ///
    /// Match-on-variant rather than equality because `GeometryError` does
    /// not derive `PartialEq`. Either the left or right id may be the
    /// surfaced one — the test accepts whichever the impl looks up first.
    #[test]
    fn execute_union_with_unknown_handle_returns_invalid_reference() {
        let mut kernel = ManifoldKernel::new();
        let result = kernel.execute(&GeometryOp::Union {
            left: GeometryHandleId(99),
            right: GeometryHandleId(100),
        });

        match result {
            Err(GeometryError::InvalidReference(GeometryHandleId(99)))
            | Err(GeometryError::InvalidReference(GeometryHandleId(100))) => {}
            other => panic!(
                "execute(Union) with unknown handles must return \
                 Err(GeometryError::InvalidReference(99 or 100)); got {other:?}"
            ),
        }
    }

    /// Pins the per-trait error variant choice for the `tessellate` lookup
    /// path: an unknown handle surfaces as
    /// `Err(TessError::InvalidHandle(handle))`, NOT
    /// `GeometryError::InvalidReference` (which is the sibling variant
    /// reserved for `execute`'s handle-lookup path).
    ///
    /// `execute_union_with_unknown_handle_returns_invalid_reference` above
    /// pins the `execute` side; this test pins the `tessellate` side so
    /// the asymmetry between the two trait surfaces (`GeometryError` vs
    /// `TessError`) is locked in. A regression that unifies the two error
    /// types or reroutes `tessellate` through `get_manifold` (which returns
    /// `GeometryError`) would silently change the surfaced variant.
    ///
    /// Added during amendment round 2 (was previously uncovered).
    #[test]
    fn tessellate_with_unknown_handle_returns_invalid_handle() {
        let kernel = ManifoldKernel::new();
        let result = kernel.tessellate(GeometryHandleId(99), 0.0);

        match result {
            Err(TessError::InvalidHandle(GeometryHandleId(99))) => {}
            other => panic!(
                "tessellate(GeometryHandleId(99), …) on an empty kernel must return \
                 Err(TessError::InvalidHandle(GeometryHandleId(99))); got {other:?}"
            ),
        }
    }

    /// RED for step-7 of task 3093: pins that `tessellate(handle, 0.0)`
    /// over a stored Union result returns a non-empty `Mesh` whose index
    /// count is a multiple of three.
    ///
    /// Tolerance is `0.0` because manifold meshes are exact — the
    /// underlying [`Manifold`] carries its own tolerance set at
    /// construction, and the `tessellate` boundary intentionally ignores
    /// the caller-supplied tolerance for the v0.2 path. Step-8 wires
    /// `tessellate` via `Manifold::to_mesh_f64()`.
    ///
    /// Currently fails because `tessellate` returns the stub
    /// `TessError::TessellationFailed`.
    #[cfg(feature = "test-fixtures")]
    #[test]
    fn tessellate_of_stored_union_returns_nonempty_mesh() {
        let mut kernel = ManifoldKernel::new();
        let l = kernel
            .ingest_mesh(&unit_cube_mesh([0.0, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold")
            .id;
        let r = kernel
            .ingest_mesh(&unit_cube_mesh([0.5, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold")
            .id;

        let union_handle = kernel
            .execute(&GeometryOp::Union { left: l, right: r })
            .expect("Union of two valid cubes must succeed");

        let mesh = kernel
            .tessellate(union_handle.id, 0.0)
            .expect("tessellate of stored Union must succeed");

        assert!(
            !mesh.vertices.is_empty(),
            "tessellated Union mesh must have at least one vertex",
        );
        assert!(
            !mesh.indices.is_empty(),
            "tessellated Union mesh must have at least one triangle",
        );
        assert_eq!(
            mesh.indices.len() % 3,
            0,
            "tessellated Union mesh indices must be a multiple of 3 (triangles)",
        );
        assert_eq!(
            mesh.vertices.len() % 3,
            0,
            "tessellated Union mesh vertices must be a multiple of 3 (xyz triplets)",
        );
    }

    /// PRD docs/prds/v0_2/persistent-naming-v2.md line 70: ManifoldKernel is
    /// the first concrete impl of `KernelAttributeHook`. This test pins the
    /// "ManifoldKernel opts into the hook AND is reachable through the
    /// trait-object accessor" contract — a regression that loses the override
    /// (e.g. removed `attribute_hook()` impl on ManifoldKernel) would silently
    /// fall back to the `None` default and the engine-side dispatcher would
    /// route Manifold ops to `FellThrough`, defeating the multi-kernel
    /// propagation pipeline this task builds.
    ///
    /// Bound as `&dyn GeometryKernel` (not `&ManifoldKernel`) because the
    /// engine-side dispatcher invokes the accessor through a trait object —
    /// asserting via the typed concrete reference would let an accidental
    /// `&self`/`&dyn` divergence slip through.
    #[test]
    fn manifold_kernel_advertises_attribute_hook_via_geometry_kernel_trait() {
        let kernel = ManifoldKernel::new();
        let kernel_ref: &dyn reify_ir::GeometryKernel = &kernel;
        assert!(
            kernel_ref.attribute_hook().is_some(),
            "ManifoldKernel must override `attribute_hook()` to return Some(self) — \
             enforces PRD line 70 'first concrete impl of KernelAttributeHook' contract \
             reachable through the trait-object accessor",
        );
    }

    /// PRD line 70: heavy remeshing within tolerance (and, in this v0.2 stub,
    /// the pending PRD task 9 MeshGL walk) discards attributes with a
    /// `tracing::warn!` diagnostic.
    ///
    /// Three properties are pinned by this test:
    /// (a) `propagate_attributes` returns `Ok(KernelAttributeOutcome::Discarded)`
    ///     for the v0.2 stub regardless of inputs — the trait surface model.
    /// (b) `table` is left unchanged: the stub does not write spurious entries.
    /// (c) Exactly one WARN-level event fires at the `reify_kernel_manifold::kernel`
    ///     target, matching the `Discarded` contract that hook impls emit
    ///     their own diagnostic before returning.
    ///
    /// Reuses the `CountingSubscriberBuilder` pattern from
    /// `crates/reify-eval/src/kernel_registry.rs:329-353`. Synthetic op +
    /// handle slices avoid dragging actual kernel state into the test.
    #[test]
    fn manifold_kernel_attribute_hook_returns_discarded_and_emits_warn_diagnostic() {
        use reify_test_support::CountingSubscriberBuilder;
        use reify_ir::TopologyAttributeTable;
        use std::sync::atomic::Ordering;

        let kernel = ManifoldKernel::new();
        let mut table = TopologyAttributeTable::default();
        let op = GeometryOp::Union {
            left: GeometryHandleId(1),
            right: GeometryHandleId(2),
        };
        let parents = [GeometryHandleId(1), GeometryHandleId(2)];
        let result = GeometryHandleId(3);
        let feature_id = FeatureId::new("test#realization[0]");

        let (subscriber, counters) = CountingSubscriberBuilder::new()
            .count_level(tracing::Level::WARN)
            // Qualified prefix intentionally pins the `crate::module` tracing target
            // (mirrors `target: "reify_kernel_manifold::kernel"` in the impl above).
            // If the `KernelAttributeHook` impl moves to a different submodule, update
            // both the `target:` literal in `propagate_attributes` and this prefix.
            .target_prefix("reify_kernel_manifold::kernel")
            .build();
        let warn_count = counters[&tracing::Level::WARN].clone();

        let outcome = tracing::subscriber::with_default(subscriber, || {
            kernel.propagate_attributes(&mut table, &op, &parents, result, &feature_id)
        });

        // (a) Outcome is Ok(Discarded) for the v0.2 stub.
        // Match-on-outcome rather than `assert_eq!` because `QueryError` does
        // not derive `PartialEq` (would require widening reify-types' surface
        // for a single test assertion).
        match outcome {
            Ok(KernelAttributeOutcome::Discarded) => {}
            other => panic!(
                "v0.2 Manifold stub must return Ok(Discarded) — MeshGL walk pending PRD task 9; got {other:?}"
            ),
        }

        // (b) Table is unchanged: stub does not write spurious entries.
        assert!(
            table.is_empty(),
            "Manifold Discarded path must not write to TopologyAttributeTable — \
             attributes were lost, not propagated",
        );

        // (c) Exactly one WARN event at the reify_kernel_manifold::kernel target.
        assert_eq!(
            warn_count.load(Ordering::Acquire),
            1,
            "Manifold Discarded path must emit exactly one WARN event at \
             reify_kernel_manifold::kernel target — operator visibility for the \
             intentional attribute-loss diagnostic per PRD line 70",
        );
    }

    /// Pins the architectural rule that [`ManifoldKernel`] must not misclassify
    /// its handles as `Some(BRepKind::Solid)` — a Manifold mesh belongs to the
    /// [`ReprKind::Mesh`] family, not the B-rep family, so there is no
    /// meaningful B-rep sub-shape classification and `repr` must be `None`.
    ///
    /// # Context
    ///
    /// - **Task 3179**: Resolves the BRepKind semantic abuse for non-B-rep
    ///   kernels (architectural decision to widen
    ///   `GeometryHandle.repr: BRepKind` → `Option<BRepKind>`).
    /// - **Task 3093 review esc-3093-33**: The original acknowledgement of the
    ///   semantic abuse — Manifold's `store` carried an inline comment "There
    ///   is no `BRepKind::Mesh` variant; `Solid` is the closest semantic
    ///   match", explicitly noting the misclassification.
    /// - **Architectural rule**: `BRepKind` is documented as a *B-rep
    ///   sub-shape classifier for geometry handles managed by the OCCT
    ///   kernel*. Non-B-rep kernels (Mesh/Sdf/Voxel/VolumeMesh families per
    ///   [`ReprKind`]) genuinely have no B-rep sub-shape. `None` is
    ///   structurally honest; `Some(BRepKind::Solid)` was a forced lie.
    ///   The coarse kernel-family classifier lives in [`ReprKind`], not in
    ///   `BRepKind`.
    #[cfg(feature = "test-fixtures")]
    #[test]
    fn manifold_kernel_handle_repr_is_none_for_non_brep_kernel() {
        let mut kernel = ManifoldKernel::new();
        let l = kernel
            .ingest_mesh(&unit_cube_mesh([0.0, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold")
            .id;
        let r = kernel
            .ingest_mesh(&unit_cube_mesh([0.5, 0.0, 0.0]))
            .expect("unit_cube_mesh fixture must be a valid manifold")
            .id;

        let handle = kernel
            .execute(&GeometryOp::Union { left: l, right: r })
            .expect("Union of two valid stored cubes must return Ok(GeometryHandle)");

        assert!(
            handle.repr.is_none(),
            "ManifoldKernel handles must carry `repr: None` — Manifold meshes \
             belong to ReprKind::Mesh and have no meaningful B-rep sub-shape \
             classification. See task 3179 option (b) and task 3093 review \
             esc-3093-33.",
        );
    }

    /// Pins that `GeometryKernel::ingest_mesh` default returns
    /// `Err(GeometryError::OperationFailed(_))` with the concrete kernel name
    /// and the "does not accept Mesh inputs" sentinel phrase.
    ///
    /// Uses `reify_test_support::FailingMockGeometryKernel` — a non-overriding
    /// `GeometryKernel` impl that is already an ungated dev-dep — so the test
    /// exercises the trait default directly without requiring a new dependency
    /// (e.g. `reify-kernel-fidget`). Design decision 4 (task 4047 plan.json):
    /// "Negative test reuses `FailingMockGeometryKernel` rather than
    /// `FidgetKernel`."
    ///
    /// Structural assertions:
    /// - result is `Err(GeometryError::OperationFailed(_))` (match-on-variant;
    ///   `GeometryError` does not derive `PartialEq`)
    /// - the `OperationFailed` payload contains "FailingMockGeometryKernel"
    ///   (proves `type_name::<Self>()` resolves to the *concrete* kernel name)
    /// - the payload contains "does not accept Mesh inputs"
    ///
    /// RED: fails to compile until `ingest_mesh` is added to `GeometryKernel`
    /// (step-2 of task 4047).
    #[test]
    fn ingest_mesh_on_non_overriding_kernel_returns_operation_failed_with_kernel_name() {
        let mut kernel = reify_test_support::FailingMockGeometryKernel;
        let result = kernel.ingest_mesh(&Mesh {
            vertices: vec![],
            indices: vec![],
            normals: None,
        });
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("FailingMockGeometryKernel"),
                    "OperationFailed payload must contain the concrete kernel name \
                     (via type_name::<Self>()); got: {msg:?}",
                );
                assert!(
                    msg.contains("does not accept Mesh inputs"),
                    "OperationFailed payload must contain the sentinel phrase \
                     \"does not accept Mesh inputs\"; got: {msg:?}",
                );
            }
            other => panic!(
                "ingest_mesh on a non-overriding kernel must return \
                 Err(GeometryError::OperationFailed(_)); got {other:?}",
            ),
        }
    }

    /// Pins that `GeometryKernel::ingest_mesh` returns
    /// `Err(GeometryError::OperationFailed(_))` when given an invalid
    /// (non-manifold) mesh.
    ///
    /// A single open triangle is structurally not a closed orientable manifold
    /// (it has three boundary edges with no closing surface), so
    /// `Manifold::from_mesh_f64` must reject it. Match-on-variant rather than
    /// equality because `GeometryError` does not derive `PartialEq` — mirrors
    /// `execute_union_with_unknown_handle_returns_invalid_reference`.
    ///
    /// This test does not need `#[cfg(feature = "test-fixtures")]` because it
    /// lives inside the unit `mod tests` block, which is compiled under
    /// `cfg(test)` — the gating predicate `cfg(any(test, feature =
    /// "test-fixtures"))` is satisfied by `cfg(test)` alone.
    #[test]
    fn ingest_mesh_with_invalid_mesh_returns_err_operation_failed() {
        let mut kernel = ManifoldKernel::new();
        // A single open triangle — three vertices, one triangle face.
        // Not a closed manifold: three boundary edges, no closing surface.
        // `Manifold::from_mesh_f64` requires closed orientable surfaces and
        // must fail on this input.
        let bad_mesh = Mesh {
            vertices: vec![
                0.0_f32, 0.0, 0.0, // v0
                1.0, 0.0, 0.0, // v1
                0.0, 1.0, 0.0, // v2
            ],
            indices: vec![0, 1, 2],
            normals: None,
        };

        let result = kernel.ingest_mesh(&bad_mesh);

        match result {
            Err(GeometryError::OperationFailed(msg)) => assert!(
                !msg.is_empty(),
                "OperationFailed payload must surface the manifold3d error — an empty message \
                 would hide the root cause from fixture authors debugging winding-order \
                 regressions (doc comment promises the underlying manifold3d error is surfaced)",
            ),
            other => panic!(
                "ingest_mesh with a single-triangle (non-manifold) mesh must return \
                 Err(GeometryError::OperationFailed(_)); got {other:?}"
            ),
        }
    }

    /// Pins the round-trip contract for `ManifoldKernel::ingest_mesh`: a
    /// valid closed-orientable mesh (the canonical `unit_cube_mesh` fixture)
    /// ingests without error and tessellates back to a geometrically faithful
    /// output.
    ///
    /// Assertions (per task 4047 design decision 3 — robust bbox rather than
    /// exact vertex count):
    /// - `out.vertices` and `out.indices` are non-empty
    /// - `out.vertices.len() % 3 == 0` and `out.indices.len() % 3 == 0`
    ///   (xyz triplets / triangle triplets invariant)
    /// - the axis-aligned bounding box of the round-tripped mesh matches the
    ///   input's within 1e-6 per axis (manifold weld/reindex preserves
    ///   geometry; exact vertex count is NOT asserted — see
    ///   `boolean_ops_integration.rs:59-63`).  The tolerance is 1e-6, not
    ///   1e-9, because `tessellate` returns f32 vertices whose machine epsilon
    ///   (~1.2e-7) makes 1e-9 physically unrepresentable; tightening the
    ///   assert to match the f64-layer prose in the PRD would make this test
    ///   unreliable.
    /// - bbox centroid == (0.5, 0.5, 0.5) within 1e-6 (same f32-egress
    ///   rationale; the unit cube is centred there for the `[0.0,0.0,0.0]`
    ///   origin variant)
    ///
    /// RED: `ManifoldKernel` currently inherits the trait default which
    /// returns `Err`; the first `.expect(…)` panics until step-4 adds the
    /// override.
    #[cfg(feature = "test-fixtures")]
    #[test]
    fn ingest_mesh_round_trips_unit_cube_through_manifold() {
        let initial = unit_cube_mesh([0.0, 0.0, 0.0]);
        let mut kernel = ManifoldKernel::new();

        let handle = kernel
            .ingest_mesh(&initial)
            .expect("unit_cube_mesh must ingest as a valid manifold");

        let out = kernel
            .tessellate(handle.id, 0.0)
            .expect("tessellate of ingested cube must succeed");

        // Structural invariants.
        assert!(
            !out.vertices.is_empty(),
            "tessellated mesh must have vertices",
        );
        assert!(
            !out.indices.is_empty(),
            "tessellated mesh must have indices",
        );
        assert_eq!(
            out.vertices.len() % 3,
            0,
            "vertices.len() must be a multiple of 3 (xyz triplets)",
        );
        assert_eq!(
            out.indices.len() % 3,
            0,
            "indices.len() must be a multiple of 3 (triangle triplets)",
        );

        // Bounding-box fidelity assertions.
        // Extract (min_x, min_y, min_z) and (max_x, max_y, max_z) from a
        // flat xyz-triplet slice.
        fn bbox(verts: &[f32]) -> ([f32; 3], [f32; 3]) {
            let mut mn = [f32::INFINITY; 3];
            let mut mx = [f32::NEG_INFINITY; 3];
            for chunk in verts.chunks(3) {
                for (axis, &v) in chunk.iter().enumerate() {
                    mn[axis] = mn[axis].min(v);
                    mx[axis] = mx[axis].max(v);
                }
            }
            (mn, mx)
        }

        let (in_min, in_max) = bbox(&initial.vertices);
        let (out_min, out_max) = bbox(&out.vertices);

        for axis in 0..3 {
            assert!(
                (out_min[axis] - in_min[axis]).abs() < 1e-6_f32,
                "bbox min[{axis}] round-trip error too large: \
                 in={}, out={} (diff={})",
                in_min[axis],
                out_min[axis],
                (out_min[axis] - in_min[axis]).abs(),
            );
            assert!(
                (out_max[axis] - in_max[axis]).abs() < 1e-6_f32,
                "bbox max[{axis}] round-trip error too large: \
                 in={}, out={} (diff={})",
                in_max[axis],
                out_max[axis],
                (out_max[axis] - in_max[axis]).abs(),
            );
        }

        // Centroid of the unit cube (origin variant) must be (0.5, 0.5, 0.5).
        for axis in 0..3 {
            let centroid = (out_min[axis] + out_max[axis]) / 2.0;
            assert!(
                (centroid - 0.5).abs() < 1e-6_f32,
                "bbox centroid[{axis}] must be 0.5 for the unit cube; \
                 got {centroid}",
            );
        }
    }

    /// Pins that `ManifoldKernel::query(GeometryQuery::Distance{from,to})`
    /// returns `Ok(Value::Real(d))` with `d ≈ 4.0` for two disjoint unit
    /// cubes at [0,0,0] and [5,0,0].
    ///
    /// `unit_cube_mesh([dx,dy,dz])` spans [dx, dx+1]³, so the cube at
    /// [0,0,0] occupies x ∈ [0,1] and the cube at [5,0,0] occupies x ∈
    /// [5,6].  The closest vertex pair is at x=1 vs x=5, giving an exact
    /// vertex-to-vertex min distance of |5 − 1| = 4.0.
    ///
    /// RED (task 3610 step-7): `ManifoldKernel::query` currently returns
    /// `Err(QueryError::QueryFailed(STUB_MSG))` for every query variant.
    /// GREEN is delivered by step-8 which adds `queries.rs` and wires
    /// the `Distance` arm.
    ///
    /// Match-on-Ok rather than assert_eq! because `QueryError` does not
    /// derive `PartialEq`.
    #[cfg(feature = "test-fixtures")]
    #[test]
    fn query_distance_of_disjoint_cubes_returns_approx_4() {
        let mut kernel = ManifoldKernel::new();
        let from = kernel
            .ingest_mesh(&unit_cube_mesh([0.0, 0.0, 0.0]))
            .expect("unit_cube_mesh([0,0,0]) must be a valid manifold")
            .id;
        let to = kernel
            .ingest_mesh(&unit_cube_mesh([5.0, 0.0, 0.0]))
            .expect("unit_cube_mesh([5,0,0]) must be a valid manifold")
            .id;

        let result = kernel.query(&GeometryQuery::Distance { from, to });

        match result {
            Ok(Value::Real(d)) => assert!(
                (d - 4.0).abs() < 1e-9,
                "distance between unit cubes at [0,0,0] and [5,0,0] must be \
                 ≈ 4.0 (vertex-to-vertex min); got {d}",
            ),
            other => panic!(
                "query(Distance{{from,to}}) must return Ok(Value::Real(≈4.0)); \
                 got {other:?}",
            ),
        }
    }
}
