use std::collections::HashMap;
use std::fmt;

use crate::diagnostics::SourceSpan;
use crate::hash::ContentHash;
use crate::value::Value;

/// Unique identifier for a geometry handle within a kernel session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GeometryHandleId(pub u64);

impl GeometryHandleId {
    /// Sentinel value representing a failed geometry operation.
    ///
    /// Pushed into `step_handles` when `compile_geometry_op` returns `None`
    /// to maintain step index alignment so independent subsequent ops can
    /// still be attempted. No real geometry kernel allocates handle ID
    /// `u64::MAX` (kernels start from 1 and increment).
    pub const INVALID: GeometryHandleId = GeometryHandleId(u64::MAX);

    /// Compute a content hash for incremental caching.
    /// Domain-separated with tag byte [11] followed by the id as le_bytes.
    /// This serves as a proxy hash since OCCT shapes can't be hashed directly.
    pub fn content_hash(&self) -> ContentHash {
        debug_assert_ne!(self.0, u64::MAX, "INVALID handle must not be hashed");
        let mut buf = [0u8; 9];
        buf[0] = 11;
        buf[1..].copy_from_slice(&self.0.to_le_bytes());
        ContentHash::of(&buf)
    }
}

/// An opaque handle to a geometry object managed by a kernel.
#[derive(Debug, Clone)]
pub struct GeometryHandle {
    pub id: GeometryHandleId,
    pub repr: ReprKind,
}

/// What kind of geometric representation this handle holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReprKind {
    /// B-rep solid.
    Solid,
    /// Shell (open or closed).
    Shell,
    /// Wire.
    Wire,
    /// Compound of multiple shapes.
    Compound,
    /// Single edge — produced by `extract_edges`.
    ///
    /// Distinct from `Wire` (which is a sequence of edges joined end-to-end).
    Edge,
    /// Single face — produced by `extract_faces`.
    ///
    /// Distinct from `Shell` (which is a collection of faces, possibly closed).
    Face,
}

/// Operations that can be sent to a geometry kernel.
#[derive(Debug, Clone)]
pub enum GeometryOp {
    /// Create a box primitive centered at origin.
    Box {
        width: Value,
        height: Value,
        depth: Value,
    },
    /// Create a cylinder primitive along Z axis.
    Cylinder { radius: Value, height: Value },
    /// Create a sphere primitive.
    Sphere { radius: Value },
    /// Create a tube (hollow cylinder) along Z axis.
    ///
    /// Composed at the kernel layer as `boolean_cut(make_cylinder(outer_r, h),
    /// make_cylinder(inner_r, h))`. Requires `inner_r < outer_r`.
    Tube {
        outer_r: Value,
        inner_r: Value,
        height: Value,
    },
    /// Boolean union.
    Union {
        left: GeometryHandleId,
        right: GeometryHandleId,
    },
    /// Boolean difference (left - right).
    Difference {
        left: GeometryHandleId,
        right: GeometryHandleId,
    },
    /// Boolean intersection.
    Intersection {
        left: GeometryHandleId,
        right: GeometryHandleId,
    },
    /// Fillet (round) edges by radius.
    Fillet {
        target: GeometryHandleId,
        radius: Value,
    },
    /// Chamfer edges by distance.
    Chamfer {
        target: GeometryHandleId,
        distance: Value,
    },
    /// Translate by vector (dx, dy, dz in meters).
    Translate {
        target: GeometryHandleId,
        dx: f64,
        dy: f64,
        dz: f64,
    },
    /// Rotate around axis by angle.
    Rotate {
        target: GeometryHandleId,
        axis: [f64; 3],
        angle_rad: f64,
    },
    /// Uniform scale by factor around a center point.
    Scale {
        target: GeometryHandleId,
        factor: f64,
    },
    /// Rotate around an arbitrary axis passing through a given point.
    RotateAround {
        target: GeometryHandleId,
        point: [f64; 3],
        axis: [f64; 3],
        angle_rad: f64,
    },
    /// Create a linear pattern of copies along a direction.
    LinearPattern {
        target: GeometryHandleId,
        direction: [f64; 3],
        count: usize,
        spacing: Value,
    },
    /// Create a circular pattern of copies around an axis.
    CircularPattern {
        target: GeometryHandleId,
        axis_origin: [f64; 3],
        axis_dir: [f64; 3],
        count: usize,
        angle: Value,
    },
    /// Mirror a shape across a plane.
    Mirror {
        target: GeometryHandleId,
        plane_origin: [f64; 3],
        plane_normal: [f64; 3],
    },
    /// Create a 2D grid pattern of copies along two directions.
    LinearPattern2D {
        target: GeometryHandleId,
        direction1: [f64; 3],
        count1: usize,
        spacing1: Value,
        direction2: [f64; 3],
        count2: usize,
        spacing2: Value,
    },
    /// Create copies at user-specified translation offsets.
    ArbitraryPattern {
        target: GeometryHandleId,
        transforms: Vec<[f64; 3]>,
    },
    /// Loft through a sequence of profiles.
    Loft { profiles: Vec<GeometryHandleId> },
    /// Extrude a 2D profile along Z axis by distance.
    Extrude {
        profile: GeometryHandleId,
        distance: Value,
    },
    /// Create a revolved solid by rotating a profile around an axis.
    Revolve {
        profile: GeometryHandleId,
        axis_origin: [f64; 3],
        axis_dir: [f64; 3],
        angle_rad: f64,
    },
    /// Sweep a profile along a path wire (BRepOffsetAPI_MakePipe).
    Sweep {
        profile: GeometryHandleId,
        path: GeometryHandleId,
    },
    /// Create a pipe along `path` with circular cross-section of `radius`.
    ///
    /// Composed at the kernel layer as `make_pipe(make_circle_face(radius, 0.0),
    /// path)`. The circle cross-section is a private kernel-internal detail.
    ///
    /// # Orientation constraint
    ///
    /// The circular cross-section is a face in the **XY plane at z=0**
    /// (i.e. its normal is +Z). `BRepOffsetAPI_MakePipe` expects the
    /// profile's plane to align with the path's start-tangent; only paths
    /// whose start-tangent is approximately **+Z** (within 1e-6) are
    /// accepted.
    ///
    /// Paths whose start-tangent is not aligned with +Z are rejected at
    /// `execute` with `GeometryError::OperationFailed`. Callers needing
    /// arbitrary path orientations should use `Sweep { profile, path }`
    /// directly, supplying an explicit profile wire already aligned to
    /// the desired frame.
    ///
    /// The `kernel_pipe_non_z_start_tangent_returns_error` test locks in
    /// this contract over +X, +Y, and arc-in-XY-plane paths.
    ///
    /// **Future work:** General start-tangent reorientation (automatically
    /// aligning the profile face to the path's local frame) is deferred;
    /// see option (a) from the task-2095 review.
    Pipe {
        path: GeometryHandleId,
        radius: Value,
    },
    /// Extrude a 2D profile symmetrically along Z axis — distance/2 each way.
    ///
    /// The extruded solid's centroid (along the extrusion direction) aligns
    /// with the original profile's centroid. Implemented as a
    /// `make_prism(profile, 0, 0, distance)` followed by a
    /// `translate_shape(result, 0, 0, -distance/2)`.
    ExtrudeSymmetric {
        profile: GeometryHandleId,
        distance: Value,
    },
    /// Sweep a profile along a spine path, with an auxiliary guide wire
    /// constraining orientation (BRepOffsetAPI_MakePipeShell + SetMode(aux, false)).
    SweepGuided {
        profile: GeometryHandleId,
        path: GeometryHandleId,
        guide: GeometryHandleId,
    },
    /// Loft through multiple profile sections with one or more guide wires.
    ///
    /// Uses BRepOffsetAPI_MakePipeShell: first guide becomes the spine, each
    /// profile is added as a section via `Add`, and an optional second guide
    /// is applied via `SetMode(aux_wire, false)` as an auxiliary constraint.
    /// Requires `profiles.len() >= 2` and `guides.len() >= 1`.
    LoftGuided {
        profiles: Vec<GeometryHandleId>,
        guides: Vec<GeometryHandleId>,
    },
    /// Create a line segment wire between two points.
    LineSegment {
        x1: f64,
        y1: f64,
        z1: f64,
        x2: f64,
        y2: f64,
        z2: f64,
    },
    /// Create a circular arc wire.
    Arc {
        center: [f64; 3],
        radius: f64,
        start_angle: f64,
        end_angle: f64,
        axis: [f64; 3],
    },
    /// Create a helix wire.
    Helix {
        radius: f64,
        pitch: f64,
        height: f64,
    },
    /// Create an interpolated curve through points.
    InterpCurve {
        points: Vec<[f64; 3]>,
    },
    /// Create a Bézier curve from control points.
    BezierCurve {
        control_points: Vec<[f64; 3]>,
    },
    /// Create a NURBS curve.
    NurbsCurve {
        control_points: Vec<[f64; 3]>,
        weights: Vec<f64>,
        knots: Vec<f64>,
        degree: usize,
    },
    /// Apply draft angle to faces.
    Draft {
        target: GeometryHandleId,
        angle: Value,
        plane: GeometryHandleId,
    },
    /// Thicken a shape by offset.
    Thicken {
        target: GeometryHandleId,
        offset: Value,
    },
    /// Shell a solid (hollow it out, removing specified faces).
    Shell {
        target: GeometryHandleId,
        thickness: Value,
        faces_to_remove: Vec<usize>,
    },
}

/// Queries against geometry handles.
#[derive(Debug, Clone)]
pub enum GeometryQuery {
    /// Compute volume in m³.
    Volume(GeometryHandleId),
    /// Compute surface area in m².
    SurfaceArea(GeometryHandleId),
    /// Compute centroid position.
    Centroid(GeometryHandleId),
    /// Compute bounding box.
    BoundingBox(GeometryHandleId),
    /// Compute minimum distance between two shapes.
    Distance {
        from: GeometryHandleId,
        to: GeometryHandleId,
    },
    /// Compute moment of inertia around an axis.
    MomentOfInertia {
        handle: GeometryHandleId,
        axis: [f64; 3],
    },
    /// Find faces sharing at least one edge with the given face.
    ///
    /// `face_index` is the 0-based index into the shape's face enumeration
    /// (TopExp_Explorer order). Returns a `Value::List` of `Value::Int`
    /// global face indices, with the queried face itself excluded.
    AdjacentFaces {
        shape: GeometryHandleId,
        face_index: usize,
    },
    /// Find edges shared between two faces of the same solid.
    ///
    /// `face_a` and `face_b` are 0-based indices into the shape's face
    /// enumeration (TopExp_Explorer order). Returns a `Value::List` of
    /// `Value::Int` global edge indices. When `face_a == face_b`, returns
    /// an empty list (per design decision).
    SharedEdges {
        shape: GeometryHandleId,
        face_a: usize,
        face_b: usize,
    },
    /// Check whether a shape is watertight (closed, no free edges).
    ///
    /// Backed by `BRepCheck_Analyzer.IsValid()`. Returns `Value::Bool(true)` for
    /// valid SOLID/COMPSOLID/SHELL shapes. Returns `Value::Bool(false)` for
    /// COMPOUND, FACE, WIRE, EDGE, VERTEX (shape-type guard). COMPOUND is
    /// intentionally excluded because `BRepCheck_Analyzer.IsValid()` on a
    /// compound checks topological consistency, not closure — a compound of
    /// open faces would spuriously pass. Callers needing per-sub-shape
    /// watertightness should iterate the compound's children.
    IsWatertight(GeometryHandleId),
    /// Check whether every edge of a shape has at most 2 parent faces.
    ///
    /// Backed by walking the cached `edge_face_map`. Returns `Value::Bool(true)`
    /// iff every edge in the shape has ≤ 2 incident faces. Shapes with no face
    /// incidence (wires, edges, vertices) trivially return `true`.
    IsManifold(GeometryHandleId),
    /// Check whether all shells in a shape are consistently oriented.
    ///
    /// Backed by `ShapeAnalysis_Shell::CheckOrientedShells(shape, alsofree=false)`.
    /// Returns `Value::Bool(true)` iff every connected edge has opposite
    /// (FORWARD/REVERSED) orientations on its two incident faces. Shapes with
    /// no shells loaded (wires, isolated faces, vertices) trivially return `true`.
    IsOrientable(GeometryHandleId),
    /// Compute the center of mass for a uniform-density solid.
    ///
    /// For uniform-density solids, the center of mass coincides with the
    /// geometric centroid, so `density` is currently unused at the kernel
    /// level — the result is identical to `Centroid(handle)`. The `density`
    /// field is retained for API parity with the eventual stdlib
    /// `center_of_mass(s, ρ)` function and for forward-compatibility with
    /// non-uniform density models.
    ///
    /// Returns `Value::String` with JSON encoding `{"x":_,"y":_,"z":_}`,
    /// identical to the `Centroid` variant.
    CenterOfMass {
        handle: GeometryHandleId,
        density: f64,
    },
    /// Compute the full 3×3 inertia tensor (mass-weighted) about the centroid.
    ///
    /// Uses `BRepGProp::VolumeProperties` + `GProp_GProps::MatrixOfInertia()`.
    /// Each entry of OCCT's volume-weighted matrix is multiplied by `density`
    /// to yield the mass-weighted tensor in kg·m².
    ///
    /// Returns `Value::List(rows)` where each row is `Value::List(Vec<Value::Real>)`
    /// of three reals, in row-major order:
    /// ```text
    /// [[m11, m12, m13],
    ///  [m21, m22, m23],
    ///  [m31, m32, m33]]
    /// ```
    /// This 3-row 3-col layout matches the shape the eventual
    /// `Tensor<2, 3, MomentOfInertia>` stdlib type will expect.
    InertiaTensor {
        handle: GeometryHandleId,
        density: f64,
    },
    /// Compute the length of an edge in meters.
    ///
    /// Backed by `BRepGProp::LinearProperties` + `GProp_GProps::Mass()`.
    /// Returns `Value::Real(length_m)`. Intended for handles that name a
    /// single edge (e.g. those produced by `extract_edges`); on other
    /// shape types the result is the sum of all edge lengths in the shape.
    EdgeLength(GeometryHandleId),
    /// Compute the unit tangent of an edge at its parametric midpoint.
    ///
    /// Backed by `BRepAdaptor_Curve::D1` evaluated at
    /// `(FirstParameter + LastParameter) / 2`. Returns
    /// `Value::String` with JSON encoding `{"x":_,"y":_,"z":_}` of the
    /// normalised tangent vector (sign is the curve's parametric direction
    /// — callers needing direction-agnostic comparisons should accept
    /// either sign).
    EdgeTangent(GeometryHandleId),
    /// Compute the unit normal of a face at its centroid.
    ///
    /// Backed by `BRepGProp::SurfaceProperties` + `BRepAdaptor_Surface::D1`.
    /// Returns `Value::String` with JSON encoding `{"x":_,"y":_,"z":_}` of
    /// the normalised face normal (orientation follows the underlying
    /// surface's parametric +N — callers needing direction-agnostic
    /// comparisons should accept either sign).
    FaceNormal(GeometryHandleId),
}

/// Export formats for geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExportFormat {
    Step,
    Stl,
    Obj,
}

/// Tessellated mesh for visualization.
#[derive(Debug, Clone)]
pub struct Mesh {
    /// Vertex positions, flat [x0, y0, z0, x1, y1, z1, ...].
    pub vertices: Vec<f32>,
    /// Triangle indices, flat [i0, i1, i2, i3, i4, i5, ...].
    pub indices: Vec<u32>,
    /// Optional vertex normals, flat like vertices.
    pub normals: Option<Vec<f32>>,
}

/// Errors from geometry operations.
#[derive(Debug, Clone)]
pub enum GeometryError {
    /// Reference to a handle that doesn't exist.
    InvalidReference(GeometryHandleId),
    /// Operation failed (e.g., zero-dimension primitive).
    OperationFailed(String),
    /// Kernel initialization error.
    InitFailed(String),
}

impl fmt::Display for GeometryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GeometryError::InvalidReference(id) => {
                write!(f, "invalid geometry handle: {:?}", id)
            }
            GeometryError::OperationFailed(msg) => {
                write!(f, "geometry operation failed: {}", msg)
            }
            GeometryError::InitFailed(msg) => {
                write!(f, "geometry kernel init failed: {}", msg)
            }
        }
    }
}

impl std::error::Error for GeometryError {}

/// Errors from export operations.
#[derive(Debug, Clone)]
pub enum ExportError {
    InvalidHandle(GeometryHandleId),
    IoError(String),
    FormatError(String),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::InvalidHandle(id) => write!(f, "invalid handle for export: {:?}", id),
            ExportError::IoError(msg) => write!(f, "export I/O error: {}", msg),
            ExportError::FormatError(msg) => write!(f, "export format error: {}", msg),
        }
    }
}

impl std::error::Error for ExportError {}

/// Errors from tessellation.
#[derive(Debug, Clone)]
pub enum TessError {
    InvalidHandle(GeometryHandleId),
    TessellationFailed(String),
}

impl fmt::Display for TessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TessError::InvalidHandle(id) => write!(f, "invalid handle for tessellation: {:?}", id),
            TessError::TessellationFailed(msg) => write!(f, "tessellation failed: {}", msg),
        }
    }
}

impl std::error::Error for TessError {}

/// Errors from geometry queries.
#[derive(Debug, Clone)]
pub enum QueryError {
    InvalidHandle(GeometryHandleId),
    QueryFailed(String),
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryError::InvalidHandle(id) => write!(f, "invalid handle for query: {:?}", id),
            QueryError::QueryFailed(msg) => write!(f, "geometry query failed: {}", msg),
        }
    }
}

impl std::error::Error for QueryError {}

/// Trait for geometry kernels. Lives in reify-types for dependency inversion —
/// implemented in reify-kernel-occt, consumed by reify-eval via reify-geometry.
pub trait GeometryKernel: Send + Sync {
    /// Execute a geometry operation, returning a handle to the result.
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError>;

    /// Run a query against a handle.
    fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError>;

    /// Run a batch of queries in a single round-trip and return one
    /// `Value` per query, in the same order as `queries`.
    ///
    /// # Length invariant
    ///
    /// On success, implementations **must** return a `Vec<Value>` whose
    /// length equals `queries.len()`, with `result[i]` being the answer
    /// to `queries[i]`. Callers (e.g. the topology selectors in
    /// `reify-eval`) rely on this invariant to `zip` ids with values
    /// without an explicit re-check; a misbehaving impl that returns a
    /// shorter `Vec` would silently truncate consumer results. Defensive
    /// callers may still verify `result.len() == queries.len()` and
    /// surface `QueryError::QueryFailed` on a kernel-side contract
    /// violation.
    ///
    /// The default implementation simply forwards to `query` per element
    /// and collects via `Result<Vec<_>, _>`'s fail-fast `FromIterator`
    /// impl: it **returns the first `QueryError` encountered; remaining
    /// queries are not issued.** This default trivially preserves the
    /// length invariant because a successful `Result<Vec<_>, _>::collect`
    /// produces exactly one `Value` per source `Result::Ok`.
    ///
    /// Channel-routed kernels (e.g. `OcctKernelHandle`) override this to
    /// batch the actor-channel hop and the underlying FFI work into a
    /// single send/recv round-trip, eliminating the N+1 overhead that
    /// per-element `query` incurs in tight selector loops.
    ///
    /// Overriding impls should call [`debug_assert_query_many_invariant`]
    /// before returning the reply so a kernel-side path that violates the
    /// length contract panics in dev/test builds rather than silently
    /// truncating consumer results.
    fn query_many(&self, queries: &[GeometryQuery]) -> Result<Vec<Value>, QueryError> {
        queries.iter().map(|q| self.query(q)).collect()
    }

    /// Export a handle to the given format, writing to the provided writer.
    fn export(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError>;

    /// Tessellate a handle into a mesh.
    fn tessellate(&self, handle: GeometryHandleId, tolerance: f64) -> Result<Mesh, TessError>;

    /// Extract the unique edges of a shape, storing each as a new handle.
    ///
    /// Returns a `Vec<GeometryHandleId>` where each id names a freshly-stored
    /// edge sub-shape (with `ReprKind::Edge`). The ordering follows the
    /// kernel's canonical `TopExp::MapShapes(.., TopAbs_EDGE, ..)` enumeration,
    /// deduplicated by `TopoDS_Shape::IsSame`.
    ///
    /// Default implementation returns
    /// `Err(QueryError::QueryFailed("topology extraction not supported by this kernel"))`,
    /// keeping non-OCCT kernels (mocks, stubs) compiling without per-impl edits.
    fn extract_edges(
        &mut self,
        _handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        Err(QueryError::QueryFailed(
            "topology extraction not supported by this kernel".into(),
        ))
    }

    /// Extract the unique faces of a shape, storing each as a new handle.
    ///
    /// Returns a `Vec<GeometryHandleId>` where each id names a freshly-stored
    /// face sub-shape (with `ReprKind::Face`). The ordering follows the
    /// kernel's canonical `TopExp::MapShapes(.., TopAbs_FACE, ..)` enumeration,
    /// deduplicated by `TopoDS_Shape::IsSame`.
    ///
    /// Default implementation returns
    /// `Err(QueryError::QueryFailed("topology extraction not supported by this kernel"))`,
    /// keeping non-OCCT kernels (mocks, stubs) compiling without per-impl edits.
    fn extract_faces(
        &mut self,
        _handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        Err(QueryError::QueryFailed(
            "topology extraction not supported by this kernel".into(),
        ))
    }
}

/// Debug-build invariant check for kernel implementors that override
/// [`GeometryKernel::query_many`]. Asserts the kernel's reply has one
/// element per input query so a buggy actor-channel or FFI path is caught
/// in tests rather than silently truncating consumer results via `zip`'s
/// shorter-of-two behaviour. In release builds this is a no-op
/// (`debug_assert_eq!`).
///
/// Generic in both `Q` and `R` because only the slice lengths are read —
/// overriders may call this without an explicit turbofish, and the helper
/// remains valid if `query_many` ever returns a different element type.
pub fn debug_assert_query_many_invariant<Q, R>(queries: &[Q], reply: &[R]) {
    debug_assert_eq!(
        reply.len(),
        queries.len(),
        "query_many length invariant: kernel returned {} values for {} queries",
        reply.len(),
        queries.len()
    );
}

// ─── Feature-tag IR (task 2323) ───────────────────────────────────────────────

/// Coarse classification of the geometry operation kind that produced a shape.
///
/// Intentionally decoupled from `reify-compiler`'s fine-grained sub-kind enums
/// (`PrimitiveKind`, `BooleanOp`, `ModifyKind`, …) so that `reify-types` does
/// not gain a reverse dependency on `reify-compiler`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepKind {
    /// A primitive creation op (box, cylinder, sphere, tube).
    Primitive,
    /// A boolean op (union, difference, intersection).
    Boolean,
    /// A modify op (fillet, chamfer, shell, draft, thicken).
    Modify,
    /// A transform op (translate, rotate, scale, mirror, …).
    Transform,
    /// A pattern op (linear, circular, mirror pattern).
    Pattern,
    /// A sweep op (extrude, revolve, loft, pipe, …).
    Sweep,
    /// A curve construction op (line_segment, arc, helix, …).
    Curve,
}

/// A feature tag attached to a compiler-generated geometry op.
///
/// `source_span` identifies the **enclosing realization** (the `let`-binding
/// that produced this op stream); all ops within one realization share the same
/// span.  The only distinguisher *within* a realization is `sub_index`, which
/// is a zero-based position in the op sequence.
///
/// **Stability caveat:** `sub_index` is fragile under op insertion or
/// reordering.  A follow-up task can improve stability by threading per-op
/// source spans through `CompiledGeometryOp`; for now, consumers should treat
/// `(source_span, sub_index)` as stable only when the program text is
/// unchanged.
///
/// `source_span` stores the full `SourceSpan` rather than a line number so
/// that consumers with access to the source text can derive a line/column via
/// `byte_offset_to_line_col(source, span.start)` — the same pattern used
/// everywhere else in the codebase (`Diagnostic::span`, `RealizationDecl::span`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureTag {
    /// Byte-offset span of the enclosing realization in source.
    pub source_span: SourceSpan,
    /// Coarse classification of the op.
    pub step_kind: StepKind,
    /// Zero-based index of this op within the realization's op stream.
    pub sub_index: u32,
}

/// Runtime table mapping geometry handle ids to their originating feature tags.
///
/// Populated by `Engine::execute_realization_ops` as each op succeeds.
/// Keyed by `GeometryHandleId` so topology selectors can record per-edge /
/// per-face tags derived from a parent solid's tag.
#[derive(Debug, Default)]
pub struct FeatureTagTable {
    entries: HashMap<GeometryHandleId, FeatureTag>,
}

impl FeatureTagTable {
    /// Record that geometry handle `id` was produced by `tag`.
    ///
    /// Overwrites any prior entry for the same id (callers should avoid
    /// duplicates, but this is not a hard error — the most recent tag wins).
    pub fn record(&mut self, id: GeometryHandleId, tag: FeatureTag) {
        self.entries.insert(id, tag);
    }

    /// Look up the tag for a given geometry handle, if any.
    pub fn lookup(&self, id: GeometryHandleId) -> Option<&FeatureTag> {
        self.entries.get(&id)
    }

    /// Number of entries currently in the table.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the table has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ----------------------------------------------------------------------
// v0.2 persistent-naming-v2 (task 2590)
//
// New attribute-based topology naming primitives. Coexist with the v0.1
// `FeatureTag`/`FeatureTagTable` machinery above; the v0.1 path stays in
// place until selector resolution swaps over (task 2 / #2570) and per-op
// auto-population lands across tasks 5-8. See
// `docs/prds/v0_2/persistent-naming-v2.md` lines 46-87 for the design
// reference.
// ----------------------------------------------------------------------

/// Path-based feature identifier for v0.2 persistent naming.
///
/// Wraps a §6.5 path string (e.g. `Bracket#realization[0]`). Constructed
/// directly from any node-identity type via `From` impls; tasks 5-8 will
/// add more From-impls as additional feature-producing node kinds appear.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FeatureId(String);

impl FeatureId {
    /// Construct a `FeatureId` from any string-like value.
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }
}

impl fmt::Display for FeatureId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&crate::identity::RealizationNodeId> for FeatureId {
    fn from(id: &crate::identity::RealizationNodeId) -> Self {
        Self(id.to_string())
    }
}

/// Cap orientation for the `Role::Cap` variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CapKind {
    Top,
    Bottom,
}

/// Role of a topology entity within its originating feature.
///
/// The minimal initial set per PRD line 56. Tasks 5-8 (sweeps, primitives,
/// local features, booleans) will add per-op variants here as a closed
/// extension — there is intentionally no `Other(String)` escape hatch so
/// that selector-resolution exhaustive matching remains auditable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// A cap face (`top` / `bottom`) of the feature, e.g. extrude end caps.
    Cap(CapKind),
    /// A side (lateral) face of the feature.
    Side,
    /// An edge created by the feature's construction (e.g. cap-to-side
    /// boundary edges of an extrude).
    NewEdge,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_handle_id_content_hash_deterministic() {
        let h1 = GeometryHandleId(42).content_hash();
        let h2 = GeometryHandleId(42).content_hash();
        assert_eq!(h1, h2);

        let h3 = GeometryHandleId(0).content_hash();
        let h4 = GeometryHandleId(0).content_hash();
        assert_eq!(h3, h4);
    }

    #[test]
    fn geometry_handle_id_content_hash_distinct() {
        let h1 = GeometryHandleId(0).content_hash();
        let h2 = GeometryHandleId(1).content_hash();
        // Use u64::MAX - 1 (not INVALID = u64::MAX) to avoid triggering the
        // debug_assert in content_hash() while still proving distinctness.
        let h3 = GeometryHandleId(u64::MAX - 1).content_hash();

        assert_ne!(h1, h2);
        assert_ne!(h1, h3);
        assert_ne!(h2, h3);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "INVALID handle must not be hashed")]
    fn geometry_handle_id_content_hash_invalid_panics() {
        let _ = GeometryHandleId::INVALID.content_hash();
    }

    #[test]
    fn geometry_op_revolve_variant_exists() {
        let op = GeometryOp::Revolve {
            profile: GeometryHandleId(1),
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            angle_rad: std::f64::consts::TAU,
        };
        match &op {
            GeometryOp::Revolve {
                profile,
                axis_origin,
                axis_dir,
                angle_rad,
            } => {
                assert_eq!(*profile, GeometryHandleId(1));
                assert_eq!(*axis_origin, [0.0, 0.0, 0.0]);
                assert_eq!(*axis_dir, [0.0, 0.0, 1.0]);
                assert!((*angle_rad - std::f64::consts::TAU).abs() < 1e-15);
            }
            _ => panic!("expected Revolve variant"),
        }
    }

    #[test]
    fn geometry_op_sweep_variant_exists() {
        let op = GeometryOp::Sweep {
            profile: GeometryHandleId(1),
            path: GeometryHandleId(2),
        };
        match &op {
            GeometryOp::Sweep { profile, path } => {
                assert_eq!(*profile, GeometryHandleId(1));
                assert_eq!(*path, GeometryHandleId(2));
            }
            _ => panic!("expected Sweep variant"),
        }
    }

    #[test]
    fn geometry_handle_id_is_ordered() {
        assert!(GeometryHandleId(1) < GeometryHandleId(2));
        assert!(GeometryHandleId(5) > GeometryHandleId(3));
        assert!(GeometryHandleId(7) <= GeometryHandleId(7));
        assert!(GeometryHandleId(7) >= GeometryHandleId(7));
    }

    #[test]
    fn geometry_op_line_segment_variant_exists() {
        let op = GeometryOp::LineSegment {
            x1: 0.0, y1: 0.0, z1: 0.0,
            x2: 1.0, y2: 2.0, z2: 3.0,
        };
        match &op {
            GeometryOp::LineSegment { x1, y1, z1, x2, y2, z2 } => {
                assert_eq!((*x1, *y1, *z1), (0.0, 0.0, 0.0));
                assert_eq!((*x2, *y2, *z2), (1.0, 2.0, 3.0));
            }
            _ => panic!("expected LineSegment variant"),
        }
    }

    #[test]
    fn geometry_op_arc_variant_exists() {
        let op = GeometryOp::Arc {
            center: [1.0, 2.0, 3.0],
            radius: 5.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::FRAC_PI_2,
            axis: [0.0, 0.0, 1.0],
        };
        match &op {
            GeometryOp::Arc { center, radius, start_angle, end_angle, axis } => {
                assert_eq!(*center, [1.0, 2.0, 3.0]);
                assert_eq!(*radius, 5.0);
                assert_eq!(*start_angle, 0.0);
                assert!((*end_angle - std::f64::consts::FRAC_PI_2).abs() < 1e-15);
                assert_eq!(*axis, [0.0, 0.0, 1.0]);
            }
            _ => panic!("expected Arc variant"),
        }
    }

    #[test]
    fn geometry_op_helix_variant_exists() {
        let op = GeometryOp::Helix {
            radius: 10.0,
            pitch: 2.0,
            height: 20.0,
        };
        match &op {
            GeometryOp::Helix { radius, pitch, height } => {
                assert_eq!(*radius, 10.0);
                assert_eq!(*pitch, 2.0);
                assert_eq!(*height, 20.0);
            }
            _ => panic!("expected Helix variant"),
        }
    }

    #[test]
    fn geometry_op_interp_curve_variant_exists() {
        let op = GeometryOp::InterpCurve {
            points: vec![[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0], [3.0, 1.0, 0.0]],
        };
        match &op {
            GeometryOp::InterpCurve { points } => {
                assert_eq!(points.len(), 4);
                assert_eq!(points[0], [0.0, 0.0, 0.0]);
                assert_eq!(points[3], [3.0, 1.0, 0.0]);
            }
            _ => panic!("expected InterpCurve variant"),
        }
    }

    #[test]
    fn geometry_op_bezier_curve_variant_exists() {
        let op = GeometryOp::BezierCurve {
            control_points: vec![[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [3.0, 2.0, 0.0], [4.0, 0.0, 0.0]],
        };
        match &op {
            GeometryOp::BezierCurve { control_points } => {
                assert_eq!(control_points.len(), 4);
                assert_eq!(control_points[0], [0.0, 0.0, 0.0]);
            }
            _ => panic!("expected BezierCurve variant"),
        }
    }

    #[test]
    fn geometry_op_extrude_symmetric_variant_exists() {
        let op = GeometryOp::ExtrudeSymmetric {
            profile: GeometryHandleId(1),
            distance: Value::Real(0.01),
        };
        let cloned = op.clone();
        let debug_str = format!("{:?}", op);
        assert!(debug_str.contains("ExtrudeSymmetric"));
        match &cloned {
            GeometryOp::ExtrudeSymmetric { profile, distance } => {
                assert_eq!(*profile, GeometryHandleId(1));
                assert!((distance.as_f64().unwrap() - 0.01).abs() < 1e-15);
            }
            _ => panic!("expected ExtrudeSymmetric variant"),
        }
    }

    #[test]
    fn geometry_op_sweep_guided_variant_exists() {
        let op = GeometryOp::SweepGuided {
            profile: GeometryHandleId(1),
            path: GeometryHandleId(2),
            guide: GeometryHandleId(3),
        };
        let cloned = op.clone();
        let debug_str = format!("{:?}", op);
        assert!(debug_str.contains("SweepGuided"));
        match &cloned {
            GeometryOp::SweepGuided {
                profile,
                path,
                guide,
            } => {
                assert_eq!(*profile, GeometryHandleId(1));
                assert_eq!(*path, GeometryHandleId(2));
                assert_eq!(*guide, GeometryHandleId(3));
            }
            _ => panic!("expected SweepGuided variant"),
        }
    }

    #[test]
    fn geometry_op_loft_guided_variant_exists() {
        let op = GeometryOp::LoftGuided {
            profiles: vec![GeometryHandleId(1), GeometryHandleId(2)],
            guides: vec![GeometryHandleId(3)],
        };
        let cloned = op.clone();
        let debug_str = format!("{:?}", op);
        assert!(debug_str.contains("LoftGuided"));
        match &cloned {
            GeometryOp::LoftGuided { profiles, guides } => {
                assert_eq!(profiles.len(), 2);
                assert_eq!(profiles[0], GeometryHandleId(1));
                assert_eq!(profiles[1], GeometryHandleId(2));
                assert_eq!(guides.len(), 1);
                assert_eq!(guides[0], GeometryHandleId(3));
            }
            _ => panic!("expected LoftGuided variant"),
        }
    }

    #[test]
    fn geometry_op_tube_variant_exists() {
        let op = GeometryOp::Tube {
            outer_r: Value::Real(0.010),
            inner_r: Value::Real(0.005),
            height: Value::Real(0.020),
        };
        let cloned = op.clone();
        let debug_str = format!("{:?}", op);
        assert!(debug_str.contains("Tube"));
        match &cloned {
            GeometryOp::Tube {
                outer_r,
                inner_r,
                height,
            } => {
                assert!((outer_r.as_f64().unwrap() - 0.010).abs() < 1e-15);
                assert!((inner_r.as_f64().unwrap() - 0.005).abs() < 1e-15);
                assert!((height.as_f64().unwrap() - 0.020).abs() < 1e-15);
            }
            _ => panic!("expected Tube variant"),
        }
    }

    #[test]
    fn geometry_op_pipe_variant_exists() {
        let op = GeometryOp::Pipe {
            path: GeometryHandleId(1),
            radius: Value::Real(0.002),
        };
        let cloned = op.clone();
        let debug_str = format!("{:?}", op);
        assert!(debug_str.contains("Pipe"));
        match &cloned {
            GeometryOp::Pipe { path, radius } => {
                assert_eq!(*path, GeometryHandleId(1));
                assert!((radius.as_f64().unwrap() - 0.002).abs() < 1e-15);
            }
            _ => panic!("expected Pipe variant"),
        }
    }

    #[test]
    fn geometry_op_nurbs_curve_variant_exists() {
        let op = GeometryOp::NurbsCurve {
            control_points: vec![[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0], [3.0, 1.0, 0.0]],
            weights: vec![1.0, 1.0, 1.0, 1.0],
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            degree: 3,
        };
        match &op {
            GeometryOp::NurbsCurve { control_points, weights, knots, degree } => {
                assert_eq!(control_points.len(), 4);
                assert_eq!(weights.len(), 4);
                assert_eq!(knots.len(), 8);
                assert_eq!(*degree, 3);
            }
            _ => panic!("expected NurbsCurve variant"),
        }
    }

    #[test]
    fn geometry_kernel_query_many_default_forwards_to_query() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        /// A minimal in-test `GeometryKernel` that records every `query` call
        /// and replies with a fixed `Value::Real(42.0)`. It implements only the
        /// abstract `query` method — every other trait member uses its
        /// not-supported default or a stub `unimplemented!()` — so we can
        /// observe whether `query_many`'s default delegates to `query` per
        /// element and preserves order. `AtomicUsize` keeps the kernel
        /// `Send + Sync` (a bound the trait requires) without external locks.
        struct CountingKernel {
            query_calls: AtomicUsize,
            reply: Value,
        }

        impl GeometryKernel for CountingKernel {
            fn execute(
                &mut self,
                _op: &GeometryOp,
            ) -> Result<GeometryHandle, GeometryError> {
                unimplemented!("CountingKernel only supports query")
            }

            fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
                self.query_calls.fetch_add(1, Ordering::SeqCst);
                Ok(self.reply.clone())
            }

            fn export(
                &self,
                _handle: GeometryHandleId,
                _format: ExportFormat,
                _writer: &mut dyn std::io::Write,
            ) -> Result<(), ExportError> {
                unimplemented!("CountingKernel only supports query")
            }

            fn tessellate(
                &self,
                _handle: GeometryHandleId,
                _tolerance: f64,
            ) -> Result<Mesh, TessError> {
                unimplemented!("CountingKernel only supports query")
            }
        }

        let kernel = CountingKernel {
            query_calls: AtomicUsize::new(0),
            reply: Value::Real(42.0),
        };

        // (1) Two-element batch: returns ordered Values, calls `query` exactly twice.
        let queries = vec![
            GeometryQuery::Volume(GeometryHandleId(1)),
            GeometryQuery::SurfaceArea(GeometryHandleId(2)),
        ];
        let result = kernel
            .query_many(&queries)
            .expect("query_many should succeed");
        assert_eq!(result.len(), 2, "query_many should return one Value per query");
        match (&result[0], &result[1]) {
            (Value::Real(a), Value::Real(b)) => {
                assert!((a - 42.0).abs() < 1e-15);
                assert!((b - 42.0).abs() < 1e-15);
            }
            other => panic!("expected two Value::Real(42.0), got {:?}", other),
        }
        assert_eq!(
            kernel.query_calls.load(Ordering::SeqCst),
            2,
            "expected exactly 2 query calls"
        );

        // (2) Empty batch: returns Ok(vec![]) with zero additional `query` calls.
        let result = kernel
            .query_many(&[])
            .expect("empty query_many should succeed");
        assert!(result.is_empty(), "empty batch should return empty Vec");
        assert_eq!(
            kernel.query_calls.load(Ordering::SeqCst),
            2,
            "empty query_many must not call query"
        );
    }

    #[test]
    fn geometry_kernel_query_many_default_fails_fast_on_first_error() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        /// In-test `GeometryKernel` whose `query` returns `Ok(...)` until the
        /// `fail_after_call` threshold (1-based) is hit, then returns
        /// `QueryError::QueryFailed` for that call. Subsequent calls would
        /// also error if reached, but the trait default's fail-fast collect
        /// must short-circuit before that — the asserted call count proves it.
        struct FailAfterKernel {
            query_calls: AtomicUsize,
            fail_after_call: usize,
            ok_reply: Value,
        }

        impl GeometryKernel for FailAfterKernel {
            fn execute(
                &mut self,
                _op: &GeometryOp,
            ) -> Result<GeometryHandle, GeometryError> {
                unimplemented!("FailAfterKernel only supports query")
            }

            fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
                let call_index = self.query_calls.fetch_add(1, Ordering::SeqCst) + 1;
                if call_index >= self.fail_after_call {
                    Err(QueryError::QueryFailed(format!(
                        "synthetic failure on call #{}",
                        call_index
                    )))
                } else {
                    Ok(self.ok_reply.clone())
                }
            }

            fn export(
                &self,
                _handle: GeometryHandleId,
                _format: ExportFormat,
                _writer: &mut dyn std::io::Write,
            ) -> Result<(), ExportError> {
                unimplemented!("FailAfterKernel only supports query")
            }

            fn tessellate(
                &self,
                _handle: GeometryHandleId,
                _tolerance: f64,
            ) -> Result<Mesh, TessError> {
                unimplemented!("FailAfterKernel only supports query")
            }
        }

        // Three queries; the kernel returns Ok on call #1 and Err on call #2.
        // The default `query_many` must short-circuit at call #2 — never
        // issuing call #3 — and return that error.
        let kernel = FailAfterKernel {
            query_calls: AtomicUsize::new(0),
            fail_after_call: 2,
            ok_reply: Value::Real(1.0),
        };
        let queries = vec![
            GeometryQuery::Volume(GeometryHandleId(1)),
            GeometryQuery::SurfaceArea(GeometryHandleId(2)),
            GeometryQuery::EdgeLength(GeometryHandleId(3)),
        ];
        let err = kernel
            .query_many(&queries)
            .expect_err("query_many must propagate the inner error");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("call #2"),
                    "expected fail-fast at call #2, got {:?}",
                    msg
                );
            }
            other => panic!("expected QueryFailed, got {:?}", other),
        }
        assert_eq!(
            kernel.query_calls.load(Ordering::SeqCst),
            2,
            "fail-fast must stop after the first Err — call #3 must not be issued"
        );
    }

    #[test]
    fn geometry_query_topology_variants_can_be_constructed_and_matched() {
        let adj = GeometryQuery::AdjacentFaces {
            shape: GeometryHandleId(1),
            face_index: 0,
        };
        match &adj {
            GeometryQuery::AdjacentFaces { shape, face_index } => {
                assert_eq!(*shape, GeometryHandleId(1));
                assert_eq!(*face_index, 0);
            }
            _ => panic!("expected AdjacentFaces variant"),
        }

        let shared = GeometryQuery::SharedEdges {
            shape: GeometryHandleId(1),
            face_a: 0,
            face_b: 1,
        };
        match &shared {
            GeometryQuery::SharedEdges {
                shape,
                face_a,
                face_b,
            } => {
                assert_eq!(*shape, GeometryHandleId(1));
                assert_eq!(*face_a, 0);
                assert_eq!(*face_b, 1);
            }
            _ => panic!("expected SharedEdges variant"),
        }
    }

    #[test]
    fn repr_kind_face_and_edge_variants_exist() {
        // Construct and pattern-match the new ReprKind::Edge variant.
        let edge_repr = ReprKind::Edge;
        match edge_repr {
            ReprKind::Edge => {}
            other => panic!("expected ReprKind::Edge, got {:?}", other),
        }

        // Construct and pattern-match the new ReprKind::Face variant.
        let face_repr = ReprKind::Face;
        match face_repr {
            ReprKind::Face => {}
            other => panic!("expected ReprKind::Face, got {:?}", other),
        }

        // Edge and Face must be distinguishable from each other and from
        // existing variants (Wire/Shell/Solid/Compound).
        assert_ne!(ReprKind::Edge, ReprKind::Face);
        assert_ne!(ReprKind::Edge, ReprKind::Wire);
        assert_ne!(ReprKind::Face, ReprKind::Shell);
        assert_ne!(ReprKind::Edge, ReprKind::Solid);
        assert_ne!(ReprKind::Face, ReprKind::Compound);

        // Construct and pattern-match the new GeometryQuery::EdgeLength variant.
        let edge_len = GeometryQuery::EdgeLength(GeometryHandleId(7));
        match &edge_len {
            GeometryQuery::EdgeLength(id) => {
                assert_eq!(*id, GeometryHandleId(7));
            }
            _ => panic!("expected EdgeLength variant"),
        }

        // Construct and pattern-match GeometryQuery::EdgeTangent.
        let edge_tan = GeometryQuery::EdgeTangent(GeometryHandleId(11));
        match &edge_tan {
            GeometryQuery::EdgeTangent(id) => {
                assert_eq!(*id, GeometryHandleId(11));
            }
            _ => panic!("expected EdgeTangent variant"),
        }

        // Construct and pattern-match GeometryQuery::FaceNormal.
        let face_norm = GeometryQuery::FaceNormal(GeometryHandleId(13));
        match &face_norm {
            GeometryQuery::FaceNormal(id) => {
                assert_eq!(*id, GeometryHandleId(13));
            }
            _ => panic!("expected FaceNormal variant"),
        }
    }

    #[test]
    fn debug_assert_query_many_invariant_passes_when_lengths_match() {
        // Empty batch: the boundary case most likely to expose an off-by-one
        // bug if the helper's comparison were inverted.
        debug_assert_query_many_invariant(
            &[] as &[GeometryQuery],
            &[] as &[Value],
        );

        // Single-element batch.
        debug_assert_query_many_invariant(
            &[GeometryQuery::Volume(GeometryHandleId(1))],
            &[Value::Real(0.0)],
        );

        // Multi-element batch.
        debug_assert_query_many_invariant(
            &[
                GeometryQuery::Volume(GeometryHandleId(1)),
                GeometryQuery::Volume(GeometryHandleId(2)),
                GeometryQuery::Volume(GeometryHandleId(3)),
            ],
            &[Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)],
        );
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "query_many length invariant")]
    fn debug_assert_query_many_invariant_panics_on_length_mismatch() {
        let queries = vec![
            GeometryQuery::Volume(GeometryHandleId(1)),
            GeometryQuery::Volume(GeometryHandleId(2)),
            GeometryQuery::Volume(GeometryHandleId(3)),
        ];
        let reply = vec![Value::Real(0.0), Value::Real(0.0)];
        debug_assert_query_many_invariant(&queries, &reply);
    }

    // ------------------------------------------------------------------
    // v0.2 persistent-naming-v2 — task 1 (#2590) tests
    // ------------------------------------------------------------------

    #[test]
    fn feature_id_constructs_and_displays_round_trip() {
        let fid = FeatureId::new("Bracket#realization[0]");
        assert_eq!(format!("{}", fid), "Bracket#realization[0]");
    }

    #[test]
    fn feature_id_from_realization_node_id_matches_display() {
        use crate::identity::RealizationNodeId;
        let node = RealizationNodeId::new("Bracket", 0);
        let fid = FeatureId::from(&node);
        assert_eq!(format!("{}", fid), format!("{}", node));
    }

    #[test]
    fn feature_id_equality_and_hash_are_path_based() {
        use std::collections::HashMap;
        let a = FeatureId::new("Foo#realization[1]");
        let b = FeatureId::new("Foo#realization[1]");
        let c = FeatureId::new("Foo#realization[2]");
        assert_eq!(a, b);
        assert_ne!(a, c);

        let mut map: HashMap<FeatureId, u32> = HashMap::new();
        map.insert(a.clone(), 7);
        // `b` has equal path => should hit the same bucket.
        assert_eq!(map.get(&b), Some(&7));
        assert_eq!(map.get(&c), None);
    }

    #[test]
    fn feature_id_clone_preserves_value() {
        let a = FeatureId::new("Bracket#realization[0]");
        let b = a.clone();
        assert_eq!(a, b);
        assert_eq!(format!("{}", a), format!("{}", b));
    }

    #[test]
    fn role_cap_top_and_bottom_are_distinct() {
        assert_ne!(Role::Cap(CapKind::Top), Role::Cap(CapKind::Bottom));
    }

    #[test]
    fn role_side_and_new_edge_are_distinct() {
        assert_ne!(Role::Side, Role::NewEdge);
    }

    #[test]
    fn role_cap_top_differs_from_side() {
        assert_ne!(Role::Cap(CapKind::Top), Role::Side);
    }

    #[test]
    fn role_debug_includes_variant_name() {
        let dbg_top = format!("{:?}", Role::Cap(CapKind::Top));
        let dbg_side = format!("{:?}", Role::Side);
        let dbg_new_edge = format!("{:?}", Role::NewEdge);
        assert!(dbg_top.contains("Cap"), "expected Cap in {dbg_top}");
        assert!(dbg_top.contains("Top"), "expected Top in {dbg_top}");
        assert!(dbg_side.contains("Side"), "expected Side in {dbg_side}");
        assert!(
            dbg_new_edge.contains("NewEdge"),
            "expected NewEdge in {dbg_new_edge}"
        );
    }

    #[test]
    fn role_clone_preserves_identity() {
        let r = Role::Cap(CapKind::Bottom);
        let s = r;
        assert_eq!(r, s);
        let copy = r.clone();
        assert_eq!(r, copy);
    }

    #[test]
    fn mod_entry_constructs_with_feature_id_and_split_index() {
        let entry = ModEntry {
            splitting_feature_id: FeatureId::new("Boss#realization[0]"),
            split_index: 3,
        };
        assert_eq!(
            entry.splitting_feature_id,
            FeatureId::new("Boss#realization[0]")
        );
        assert_eq!(entry.split_index, 3);
    }

    #[test]
    fn mod_entry_split_index_distinguishes_entries() {
        let a = ModEntry {
            splitting_feature_id: FeatureId::new("a"),
            split_index: 0,
        };
        let b = ModEntry {
            splitting_feature_id: FeatureId::new("a"),
            split_index: 1,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn mod_entry_feature_id_distinguishes_entries() {
        let a = ModEntry {
            splitting_feature_id: FeatureId::new("a"),
            split_index: 0,
        };
        let b = ModEntry {
            splitting_feature_id: FeatureId::new("b"),
            split_index: 0,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn mod_entry_clone_preserves_value() {
        let a = ModEntry {
            splitting_feature_id: FeatureId::new("a"),
            split_index: 7,
        };
        let b = a.clone();
        assert_eq!(a, b);
    }
}
