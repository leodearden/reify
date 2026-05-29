use std::collections::HashMap;
use std::fmt;

use reify_core::diagnostics::SourceSpan;
use reify_core::hash::ContentHash;
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
    /// B-rep sub-shape classification for this handle.
    ///
    /// `Some(BRepKind::*)` for B-rep kernels (OCCT / OpenCASCADE) where
    /// sub-shape classification is meaningful (Solid, Shell, Wire, Compound,
    /// Edge, Face). `None` for non-B-rep kernels (Mesh/Sdf/Voxel/VolumeMesh
    /// families per [`ReprKind`]) where no B-rep sub-shape exists.
    ///
    /// Use [`ReprKind`] for the coarse kernel-family classifier; `repr` is
    /// only populated when the kernel is a genuine B-rep kernel (OCCT).
    pub repr: Option<BRepKind>,
}

/// B-rep sub-shape classifier for geometry handles managed by the OCCT kernel.
///
/// Renamed from `ReprKind` (task 2640) to free the `ReprKind` name for the
/// multi-kernel coarse classifier (`BRep | Mesh | Sdf | Voxel`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BRepKind {
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
    /// Single vertex — produced by `extract_vertices`.
    ///
    /// Distinct from `Wire`/`Shell`/`Solid` (higher-dimensional aggregates).
    /// 0-dimensional analogue of `Edge` (1-D) / `Face` (2-D). Registered by
    /// `OcctKernel::extract_vertices` (task B) for each `TopoDS_Vertex`
    /// enumerated by `TopExp::MapShapes(.., TopAbs_VERTEX, ..)`.
    Vertex,
}

/// Multi-kernel representation family classifier.
///
/// Classifies the broad representation family a geometry handle belongs to,
/// independent of any particular kernel's internal sub-shape hierarchy.
/// Use this as the outer key of [`crate::RealizationCache`] (together with
/// `entity_id` and tolerance) to keep per-family caches isolated.
///
/// Defined in PRD `docs/prds/v0_2/multi-kernel.md` "Resolved design decisions":
/// four variants at the kernel-family level. Extensible in non-breaking minor
/// versions — match arms should always include a catch-all to remain forward-
/// compatible once this enum is stabilised.
///
/// See also [`BRepKind`] for the finer-grained B-rep sub-shape classifier
/// (Solid / Shell / Wire / Compound / Edge / Face) used by the OCCT kernel.
///
/// `Ord` / `PartialOrd` are derived (in declaration order: BRep < Mesh < Sdf
/// < Voxel < VolumeMesh) so the dispatcher can seed its BFS frontier in a
/// deterministic order via `BTreeSet<ReprKind>` even when the caller passes
/// `&HashSet<ReprKind>` of `available` reprs. Without this, the seeding loop
/// would inherit HashMap salt order and selection across multi-seed cases
/// would depend on hashing rather than the registered kernel set, breaking
/// the PRD's "Selection deterministic" contract.
///
/// `VolumeMesh` is appended last in the v0.3 extension so the existing
/// `BRep < Mesh < Sdf < Voxel` ordering stays unchanged for callers that
/// pass legacy four-variant `available` sets — kernel selection on the
/// surface-mesh path remains bit-identical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ReprKind {
    /// Boundary-representation solid (OCCT / OpenCASCADE B-rep kernel).
    BRep,
    /// Surface mesh (triangle or quad mesh, e.g. Manifold).
    Mesh,
    /// Signed-distance field / implicit surface (e.g. Fidget).
    Sdf,
    /// Volumetric voxel grid (e.g. OpenVDB).
    Voxel,
    /// Volumetric tetrahedral mesh (e.g. Gmsh HXT). Distinct from [`ReprKind::Mesh`]
    /// (boundary-only triangulation) — `VolumeMesh` carries interior tet
    /// elements for FEA assembly. Produced by the v0.3 surface→volume
    /// meshing pipeline (`reify-kernel-gmsh`).
    VolumeMesh,
}

/// Multi-kernel operation classifier.
///
/// Names every operation a geometry kernel might claim to support. The pair
/// `(Operation, ReprKind)` in [`CapabilityDescriptor::supports`] reads as
/// "this kernel can perform `Operation` and produce `ReprKind`". For ops
/// that consume a geometric input — Booleans, Modify, Transform,
/// Pattern — the `ReprKind` is **both** the input repr the kernel
/// consumes and the output repr it produces. This mirrors the dispatcher's `current_repr == demanded`
/// final-stage probe in `crates/reify-eval/src/dispatcher.rs`: the gate
/// fires only when the currently-realised repr already equals `demanded`,
/// which by this invariant is also the repr the kernel expects on input.
/// For ops with no geometric input — Primitives, Curves, and all eight
/// profile-consuming Sweep variants — the `ReprKind` names only the
/// produced output repr; callers signal this by passing
/// `available = {demanded}` so the BFS treats the demanded repr as
/// trivially in scope without a conversion step. All current Sweep
/// variants (SweepExtrude, SweepRevolve, etc.) consume a 2D
/// curve/profile not tracked in the `ReprKind` lattice, so e.g.
/// `(SweepExtrude, BRep)` for an OCCT adapter means "produces a BRep
/// body"; callers pass `available = {BRep}` accordingly. Curve ops
/// (CurveArc, CurveHelix, etc.) produce BRep edges/wires in OCCT (the
/// only kernel currently registering Curve ops); adapters declare
/// `(CurveArc, BRep)` and similar, and callers pass
/// `available = {demanded}` since curve inputs are scalar parameters,
/// not [`ReprKind`]-tracked geometry. (The v0.2 dispatcher tests do
/// not yet exercise Curve dispatch; this is forward-looking for
/// kernel-adapter authors.) For
/// [`Operation::Convert { from }`] entries, `from` is the
/// input repr and the second tuple element is the output repr — the only
/// shape where the two diverge. Conversions
/// across representation families are modelled here as
/// [`Operation::Convert { from }`] so that the dispatcher's BFS can expand
/// across reprs uniformly over a single feasibility table (PRD
/// `docs/prds/v0_2/multi-kernel.md` "Capability descriptor": `supports:
/// Vec<(Operation, ReprKind)>` is the single feasibility table — no separate
/// `conversions` field).
///
/// Variants enumerate the v0.1 op surface (Booleans×3, Primitives×4,
/// Modify×5, Transform×4, Pattern×5, Sweep×8, Curve×6) plus the v0.2
/// multi-kernel `Convert { from }` variant (one logical entry per source
/// `ReprKind`). Each variant is a coarse op classifier: parameters that vary
/// per call site (e.g. fillet radius, sphere centre) live on
/// [`GeometryOp`], not here. This enum is `Hash + Eq + Copy + Debug` so it
/// can act as a `HashMap`/`BTreeMap` key in the dispatcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operation {
    // ── Booleans ─────────────────────────────────────────────────────────────
    /// Boolean union of two solids/meshes/SDFs.
    BooleanUnion,
    /// Boolean difference (left − right).
    BooleanDifference,
    /// Boolean intersection.
    BooleanIntersection,

    // ── Primitives ───────────────────────────────────────────────────────────
    /// Box primitive (centred at origin, axis-aligned).
    PrimitiveBox,
    /// Cylinder primitive (along Z axis).
    PrimitiveCylinder,
    /// Sphere primitive.
    PrimitiveSphere,
    /// Tube primitive (hollow cylinder).
    PrimitiveTube,

    // ── Modify (local edits to a single shape) ──────────────────────────────
    /// Fillet (round) edges by radius.
    ModifyFillet,
    /// Chamfer edges by distance.
    ModifyChamfer,
    /// Shell (hollow out) by thickness.
    ModifyShell,
    /// Draft faces by angle.
    ModifyDraft,
    /// Thicken a surface by offset.
    ModifyThicken,

    // ── Transform (rigid / scale) ───────────────────────────────────────────
    /// Translate by vector.
    TransformTranslate,
    /// Rotate around an axis (origin-centred).
    TransformRotate,
    /// Scale (uniform or non-uniform).
    TransformScale,
    /// Rotate around an arbitrary axis.
    TransformRotateAround,

    // ── Pattern (replicate) ─────────────────────────────────────────────────
    /// Linear pattern along an axis.
    PatternLinear,
    /// Circular pattern around an axis.
    PatternCircular,
    /// Mirror across a plane.
    PatternMirror,
    /// Linear pattern along two axes (grid).
    PatternLinear2D,
    /// Arbitrary placement list.
    PatternArbitrary,

    // ── Sweep (extrude / revolve / loft / pipe) ─────────────────────────────
    /// Loft through a sequence of profiles.
    SweepLoft,
    /// Extrude a profile linearly.
    SweepExtrude,
    /// Revolve a profile around an axis.
    SweepRevolve,
    /// Sweep a profile along a path.
    SweepSweep,
    /// Symmetric extrude (both directions).
    SweepExtrudeSymmetric,
    /// Sweep with explicit guide rails.
    SweepSweepGuided,
    /// Loft with explicit guide rails.
    SweepLoftGuided,
    /// Pipe along a path.
    SweepPipe,

    // ── Curve (1D primitives) ───────────────────────────────────────────────
    /// Line segment.
    CurveLineSegment,
    /// Arc.
    CurveArc,
    /// Helix.
    CurveHelix,
    /// Interpolated curve through points.
    CurveInterpCurve,
    /// Bezier curve from control points.
    CurveBezierCurve,
    /// NURBS curve.
    CurveNurbsCurve,

    // ── Convert (representation change) ─────────────────────────────────────
    /// Convert geometry from one [`ReprKind`] family to another. The pair
    /// `(Convert { from: BRep }, Mesh)` in a kernel's `supports` table reads
    /// as "this kernel can convert BRep input to Mesh output" (e.g. OCCT
    /// tessellation). The destination repr is the second element of the
    /// `supports` tuple, not encoded here.
    Convert { from: ReprKind },
}

/// Per-kernel feasibility table for v0.2 multi-kernel dispatch.
///
/// Each entry `(op, repr)` in [`Self::supports`] declares that this kernel
/// can perform `op` and produce a result of representation family `repr`.
/// Conversions are encoded as [`Operation::Convert { from }`] so the
/// dispatcher's BFS can expand across reprs uniformly over a single table.
///
/// PRD `docs/prds/v0_2/multi-kernel.md` "Capability descriptor": the
/// descriptor is a feasibility table only — there is no `cost_hint`, no
/// `error_factor`, and no separate `conversions` field. The dispatcher in
/// `crates/reify-eval/src/dispatcher.rs` ranks plans by conversion-stage
/// count alone, with lexicographic tie-breaking on the registered kernel
/// name.
///
/// Co-located with [`ReprKind`] and [`GeometryKernel`] so that future
/// kernel-adapter crates (`-manifold`, `-fidget`, `-openvdb`) — which
/// depend on `reify-types` for the dependency-inverted `GeometryKernel`
/// trait but deliberately NOT on `reify-compiler` or `reify-eval` — can
/// construct descriptors without pulling in dispatch logic.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapabilityDescriptor {
    /// The pairs `(op, repr)` this kernel claims to support.
    ///
    /// For ops that consume a geometric input (Booleans, Modify, Transform,
    /// Pattern), `repr` is **both** the input repr the kernel consumes and
    /// the output repr it produces. The
    /// dispatcher's `current_repr == demanded` gate
    /// (in `crates/reify-eval/src/dispatcher.rs`) confirms both
    /// simultaneously: the popped repr must equal `demanded`, and by this
    /// invariant that same equality also confirms the kernel's expected input
    /// repr. A kernel that converts `Mesh` input to a `BRep` result for
    /// [`Operation::BooleanUnion`] MUST NOT declare `(BooleanUnion, BRep)` —
    /// that entry reads as BRep→BRep. Declare
    /// `(Convert { from: Mesh }, BRep)` instead and let the dispatcher chain
    /// the conversion before a BRep-native union kernel.
    ///
    /// For ops with no geometric input — Primitives, Curves, and all
    /// profile-consuming Sweep variants — `repr` names only the produced
    /// output repr; callers signal this by passing `available = {demanded}`
    /// so the BFS treats the demanded repr as trivially in scope. All
    /// current Sweep variants consume a 2D profile/curve not tracked in
    /// the `ReprKind` lattice, so e.g. `(SweepExtrude, BRep)` names only
    /// the produced body repr. Curve ops declare the produced edge/wire
    /// repr — `BRep` in OCCT (the only kernel registering Curve ops in
    /// v0.2); callers pass `available = {demanded}` since curve inputs
    /// are scalar parameters, not [`ReprKind`]-tracked geometry.
    ///
    /// For [`Operation::Convert { from }`] entries, `from` is the input repr
    /// and the second tuple element is the output repr — the only shape where
    /// the two diverge. See the [`Operation::Convert`] variant doc for an
    /// example.
    pub supports: Vec<(Operation, ReprKind)>,
}

impl CapabilityDescriptor {
    /// Return `true` iff this descriptor's [`Self::supports`] table contains
    /// the exact pair `(op, repr)`.
    ///
    /// O(n) linear scan over `self.supports`. The table is small (4 kernels
    /// × ~10–50 entries each in v0.2), and scan order does not matter
    /// because the dispatcher already enumerates kernels in lexicographic
    /// `BTreeMap` order. Hiding the storage shape behind this helper keeps
    /// callers unconcerned with whether the underlying container changes
    /// (e.g. to a `HashSet<(Operation, ReprKind)>` for larger tables).
    pub fn supports(&self, op: Operation, repr: ReprKind) -> bool {
        self.supports.iter().any(|&(o, r)| o == op && r == repr)
    }

    /// Return `true` iff at least one entry's *output* repr — the second tuple
    /// element — equals `repr`.
    ///
    /// For [`Operation::Convert { from }`] entries the `from` field encodes the
    /// *input* repr; only the second tuple element (the produced output repr) is
    /// inspected here.  Concretely, a tessellation-only kernel declaring
    /// `(Convert { from: BRep }, Mesh)` reports `supports_any_repr(BRep)` as
    /// **false** — BRep is the FROM input, not the produced output.
    ///
    /// O(n) linear scan over `self.supports`.  The table is small (4 kernels ×
    /// ~10–50 entries each in v0.2), so no index is maintained.  Hiding the
    /// storage shape behind this helper keeps callers unconcerned with whether
    /// the underlying container changes (e.g. to a `HashSet<(Operation,
    /// ReprKind)>` for larger tables).
    ///
    /// # Callers
    ///
    /// - `pick_lexmin_brep_kernel_in` in `reify-eval` calls this with
    ///   `repr = ReprKind::BRep` to select a BRep-capable kernel during engine
    ///   construction.
    /// - Future v0.3 dispatcher-selection may key on `Mesh`, `Sdf`, or
    ///   `VolumeMesh` using the same predicate.
    pub fn supports_any_repr(&self, repr: ReprKind) -> bool {
        self.supports.iter().any(|&(_, r)| r == repr)
    }
}

/// Static registration record for a v0.2 multi-kernel adapter.
///
/// Per the PRD `docs/prds/v0_2/multi-kernel.md` "Resolved design decisions",
/// each kernel adapter crate (`reify-kernel-occt`, future `-manifold`,
/// `-fidget`, `-openvdb`) submits one of these via `inventory::submit!{ ... }`.
/// At engine startup, `reify_eval::collect_registry()` iterates
/// [`inventory::iter::<KernelRegistration>`] and materialises a
/// `BTreeMap<String, CapabilityDescriptor>` whose lexicographic key order
/// matches the dispatcher's tie-break contract
/// (see `crates/reify-eval/src/dispatcher.rs`).
///
/// # Field shapes
///
/// - `name: &'static str` — the kernel's stable identifier, used as the
///   BTreeMap key in the dispatcher registry. Lexicographic ordering of
///   `name` provides the deterministic tie-break required by the PRD's
///   "Selection deterministic given pinned runtime configuration" contract.
/// - `descriptor: fn() -> CapabilityDescriptor` — a function pointer that
///   builds the kernel's feasibility table on demand. Returns by value
///   (owned) because `CapabilityDescriptor::supports` is `Vec<...>` and
///   `Vec::push` is non-const, so a `&'static CapabilityDescriptor` would
///   require `LazyLock` indirection. Called once per `collect_registry()`
///   invocation (at engine startup), not per geometry op.
/// - `factory: fn() -> Box<dyn GeometryKernel>` — instantiates a fresh
///   kernel handle. The new
///   `Engine::with_registered_kernel(checker)` constructor calls this
///   exactly once, after iterating the registry.
///
/// # Co-location rationale
///
/// Co-located with [`CapabilityDescriptor`] (and [`GeometryKernel`]) for the
/// same dependency-inversion reason: kernel adapter crates depend on
/// `reify-types` for the trait/types they implement, but deliberately NOT
/// on `reify-compiler` or `reify-eval`. Placing the registration record
/// here lets adapters `inventory::submit!` without taking an upward dep on
/// `reify-eval` (which is the consumer of the collected set).
///
/// # Determinism
///
/// `inventory::iter::<KernelRegistration>()` does NOT guarantee link order;
/// the consumer (`reify_eval::collect_registry`) materialises into a
/// `BTreeMap` keyed on `name` so iteration becomes lexicographic
/// regardless of link ordering. Two adapters submitting with the same
/// `name` would alias-collide on the BTreeMap key — the v0.2 design
/// expects unique names per registered kernel.
pub struct KernelRegistration {
    /// Stable identifier used as the BTreeMap key in the dispatcher
    /// registry; also the lexicographic tie-break key per the PRD.
    pub name: &'static str,
    /// Builds the kernel's feasibility table on demand. Owned return
    /// avoids const-Vec construction issues — see struct doc for the full
    /// rationale.
    pub descriptor: fn() -> CapabilityDescriptor,
    /// Instantiates a fresh kernel handle. Called by
    /// `Engine::with_registered_kernel` exactly once.
    pub factory: fn() -> Box<dyn GeometryKernel>,
}

inventory::collect!(KernelRegistration);

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
    InterpCurve { points: Vec<[f64; 3]> },
    /// Create a Bézier curve from control points.
    BezierCurve { control_points: Vec<[f64; 3]> },
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

impl GeometryOp {
    /// Stable static label for this variant — used in error messages so format
    /// strings interpolate a stable token rather than the full `Debug` print.
    ///
    /// Returning `&'static str` makes the method zero-allocation. The
    /// exhaustive `match` means adding a new `GeometryOp` variant requires
    /// adding an arm here at the same diff site; the compiler enforces
    /// this — eliminating the cross-crate drift surface where downstream
    /// kernels previously had to maintain their own copy of this table.
    pub fn kind_name(&self) -> &'static str {
        match self {
            GeometryOp::Box { .. } => "Box",
            GeometryOp::Cylinder { .. } => "Cylinder",
            GeometryOp::Sphere { .. } => "Sphere",
            GeometryOp::Tube { .. } => "Tube",
            GeometryOp::Union { .. } => "Union",
            GeometryOp::Difference { .. } => "Difference",
            GeometryOp::Intersection { .. } => "Intersection",
            GeometryOp::Fillet { .. } => "Fillet",
            GeometryOp::Chamfer { .. } => "Chamfer",
            GeometryOp::Translate { .. } => "Translate",
            GeometryOp::Rotate { .. } => "Rotate",
            GeometryOp::Scale { .. } => "Scale",
            GeometryOp::RotateAround { .. } => "RotateAround",
            GeometryOp::LinearPattern { .. } => "LinearPattern",
            GeometryOp::CircularPattern { .. } => "CircularPattern",
            GeometryOp::Mirror { .. } => "Mirror",
            GeometryOp::LinearPattern2D { .. } => "LinearPattern2D",
            GeometryOp::ArbitraryPattern { .. } => "ArbitraryPattern",
            GeometryOp::Loft { .. } => "Loft",
            GeometryOp::Extrude { .. } => "Extrude",
            GeometryOp::Revolve { .. } => "Revolve",
            GeometryOp::Sweep { .. } => "Sweep",
            GeometryOp::Pipe { .. } => "Pipe",
            GeometryOp::ExtrudeSymmetric { .. } => "ExtrudeSymmetric",
            GeometryOp::SweepGuided { .. } => "SweepGuided",
            GeometryOp::LoftGuided { .. } => "LoftGuided",
            GeometryOp::LineSegment { .. } => "LineSegment",
            GeometryOp::Arc { .. } => "Arc",
            GeometryOp::Helix { .. } => "Helix",
            GeometryOp::InterpCurve { .. } => "InterpCurve",
            GeometryOp::BezierCurve { .. } => "BezierCurve",
            GeometryOp::NurbsCurve { .. } => "NurbsCurve",
            GeometryOp::Draft { .. } => "Draft",
            GeometryOp::Thicken { .. } => "Thicken",
            GeometryOp::Shell { .. } => "Shell",
        }
    }
}

/// Default tolerance (in metres) used when testing whether a world-space point
/// lies on a shape, matching OCCT's `Precision::Confusion()` (~1e-7 m).
///
/// This is the single source of truth for the tolerance value shared between:
/// - [`crate::GeometryQuery::PointOnShape`] — the kernel query variant whose
///   `tolerance` field is populated with this value by the dispatcher.
/// - `OcctKernel::point_on_shape` (`reify-kernel-occt/src/lib.rs`) — the
///   kernel-side implementation that interprets the tolerance.
/// - `try_eval_topology_selector` IsOn arm (`reify-eval/src/geometry_ops.rs`)
///   — the dispatcher that passes this constant as the `tolerance` field.
///
/// All consumers (dispatcher, stubs, integration tests) import this constant
/// directly from `reify_types`.  Drift-against-OCCT pinning lives in
/// `reify-kernel-occt`'s private test module
/// (`default_point_on_shape_tolerance_m_pins_occt_precision_confusion`), which
/// calls `Precision::Confusion()` via FFI and asserts equality at runtime.
///
/// A future `is_on(point, geometry, tol: Length)` 3-arg overload (PRD §3.9)
/// will allow callers to pass a custom tolerance; this constant remains the
/// no-tolerance-argument default.
pub const DEFAULT_POINT_ON_SHAPE_TOLERANCE_M: f64 = 1e-7;

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
    ///
    /// Dual-use: backs both the geometry-level `min_clearance` / `Distance`
    /// queries and the kinematic-constraint helpers `interferes` /
    /// `interferes_with` / `min_clearance` (PRD task 8 / task 2531). The
    /// kinematic helpers classify `Distance ≤ 0` as "intersecting" and
    /// share the same OCCT primitive (`BRepExtrema_DistShapeShape`) — see
    /// `reify_eval::geometry_ops::try_eval_kinematic_query`.
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
    /// Classify the underlying surface of a face by its OCCT
    /// `BRepAdaptor_Surface::GetType()` (`GeomAbs_*`) result.
    ///
    /// Returns `Value::String` whose payload is the canonical surface-kind
    /// name (`"Plane"`, `"Cylinder"`, `"Cone"`, `"Sphere"`, `"Torus"`,
    /// `"BezierSurface"`, `"BSplineSurface"`, `"OffsetSurface"`, or
    /// `"Other"`). Decoded by the Rust caller into [`FaceSurfaceKind`] via
    /// `TryFrom<&str>`. The string-based wire format is intentional: cxx
    /// bridge does not natively support shared enums with a fixed tag set,
    /// and a canonical string is self-documenting and forward-compatible
    /// with new OCCT GeomAbs variants.
    ///
    /// Powers PRD line 78's `%Plane`/`%Cylinder`/… geometry-type filters
    /// via `selector_vocabulary_v2::faces_by_surface_kind`.
    FaceSurfaceKind(GeometryHandleId),
    /// Classify the underlying curve of an edge by its OCCT
    /// `BRepAdaptor_Curve::GetType()` (`GeomAbs_*`) result.
    ///
    /// Returns `Value::String` whose payload is the canonical curve-kind
    /// name (`"Line"`, `"Circle"`, `"Ellipse"`, `"Hyperbola"`, `"Parabola"`,
    /// `"BezierCurve"`, `"BSplineCurve"`, `"OffsetCurve"`, or `"Other"`).
    /// Decoded by the Rust caller into [`EdgeCurveKind`] via
    /// `TryFrom<&str>`.
    ///
    /// Powers PRD line 78's `%Line`/`%Circle`/… geometry-type filters
    /// via `selector_vocabulary_v2::edges_by_curve_kind`.
    EdgeCurveKind(GeometryHandleId),
    /// List the faces that own a given edge of a solid (the edge's "ancestor"
    /// faces in topology terms).
    ///
    /// `edge_index` is the 0-based index into the shape's edge enumeration
    /// (canonical `TopExp::MapShapes(.., TopAbs_EDGE, ..)` order — same as
    /// `extract_edges`). Returns a `Value::List` of `Value::Int` global face
    /// indices into the canonical face enumeration. For a manifold solid every
    /// edge has exactly two ancestor faces, but the kernel does not enforce
    /// this — degenerate edges may surface 1 or > 2.
    ///
    /// Powers PRD line 81's `ancestors(edge)` topological walk via
    /// `selector_vocabulary_v2::ancestor_faces_of_edge`.
    AncestorFacesOfEdge {
        shape: GeometryHandleId,
        edge_index: usize,
    },
    /// Recover the parent body handle of a sub-shape produced by
    /// [`GeometryKernel::extract_edges`] / [`GeometryKernel::extract_faces`].
    ///
    /// The kernel records the parent on every `extract_*` call so any
    /// sub-handle can answer "what solid did I come from?" without
    /// re-extraction. Returns a `Value::Int(parent_id.0 as i64)`; the
    /// caller decodes back into a `GeometryHandleId`. A handle without a
    /// recorded parent (e.g. one produced directly by `execute`) surfaces
    /// as `QueryError::QueryFailed`.
    ///
    /// Powers PRD line 81's `owner_body(sub)` topological walk via
    /// `selector_vocabulary_v2::owner_body_of`.
    OwnerBody(GeometryHandleId),
    /// Project an arbitrary world-space point onto a shape and return the
    /// closest surface (or curve / vertex) point.
    ///
    /// Backed by `BRepExtrema_DistShapeShape` between the geometry handle and
    /// a `Vertex`-shape constructed from `(px, py, pz)`. Returns
    /// `Value::String` with JSON encoding `{"x":_,"y":_,"z":_}`, identical to
    /// the `Centroid` / `FaceNormal` / `EdgeTangent` wire format. The
    /// dispatcher decodes back into `Value::Point(vec![length(x), length(y),
    /// length(z)])` via the existing
    /// `reify_eval::topology_selectors::parse_xyz_value` helper.
    ///
    /// Powers the v0.1 stdlib helper
    /// `closest_point<G: Geometry>(point: Point3<Length>, geometry: G) ->
    /// Point3<Length>` (PRD §3.9). Eval-time wiring lives in
    /// `reify_eval::geometry_ops::try_eval_topology_selector`.
    ClosestPointOnShape {
        handle: GeometryHandleId,
        px: f64,
        py: f64,
        pz: f64,
    },
    /// Test whether a world-space point lies on (or inside) a shape, within a
    /// kernel-supplied tolerance.
    ///
    /// Backed by `BRepExtrema_DistShapeShape` between the handle and a
    /// `Vertex` built from `(px, py, pz)`; "on" is `distance ≤ tolerance`.
    /// Returns `Value::Bool`. The OCCT `Precision::Confusion()` (~1e-7),
    /// exposed as [`DEFAULT_POINT_ON_SHAPE_TOLERANCE_M`], is the recommended
    /// default, supplied by the dispatcher; an explicit
    /// `is_on(point, geometry, tol: Length)` overload is deferred per PRD §3.9.
    ///
    /// Note: the underlying primitive returns `true` for any interior solid
    /// point at any positive tolerance (because the closest point on a
    /// closed solid is the point itself once it's inside) — this is the v0.1
    /// contract and is documented at the kernel-side `point_on_shape`
    /// rustdoc.
    ///
    /// Powers the v0.1 stdlib helper
    /// `is_on<G: Geometry>(point: Point3<Length>, geometry: G) -> Bool`.
    PointOnShape {
        handle: GeometryHandleId,
        px: f64,
        py: f64,
        pz: f64,
        tolerance: f64,
    },
    /// Compute the unsigned dihedral angle between two surfaces (faces) of a
    /// solid (or two distinct solids) in radians ∈ `[0, π]`.
    ///
    /// Backed by `BRepAdaptor_Surface::D1` evaluated at each face's centroid,
    /// taking `acos(|n_a · n_b|)`-style absolute-cos to keep the result
    /// orientation-agnostic. Returns `Value::Real(rad)`; the eval-side
    /// dispatcher wraps as `Value::angle(rad)`.
    ///
    /// Powers the v0.1 stdlib helper
    /// `angle_between_surfaces(a: Surface, b: Surface) -> Angle` (PRD §3.9).
    SurfaceAngle {
        face_a: GeometryHandleId,
        face_b: GeometryHandleId,
    },
}

impl GeometryQuery {
    /// Stable static label for this variant — used in error messages so format
    /// strings interpolate a stable token rather than the full `Debug` print.
    ///
    /// Returning `&'static str` makes the method zero-allocation. The
    /// exhaustive `match` means adding a new `GeometryQuery` variant requires
    /// adding an arm here at the same diff site; the compiler enforces
    /// this — eliminating the cross-crate drift surface where downstream
    /// kernels previously had to maintain their own copy of this table.
    pub fn kind_name(&self) -> &'static str {
        match self {
            GeometryQuery::Volume(_) => "Volume",
            GeometryQuery::SurfaceArea(_) => "SurfaceArea",
            GeometryQuery::Centroid(_) => "Centroid",
            GeometryQuery::BoundingBox(_) => "BoundingBox",
            GeometryQuery::Distance { .. } => "Distance",
            GeometryQuery::MomentOfInertia { .. } => "MomentOfInertia",
            GeometryQuery::AdjacentFaces { .. } => "AdjacentFaces",
            GeometryQuery::AncestorFacesOfEdge { .. } => "AncestorFacesOfEdge",
            GeometryQuery::SharedEdges { .. } => "SharedEdges",
            GeometryQuery::IsWatertight(_) => "IsWatertight",
            GeometryQuery::IsManifold(_) => "IsManifold",
            GeometryQuery::IsOrientable(_) => "IsOrientable",
            GeometryQuery::CenterOfMass { .. } => "CenterOfMass",
            GeometryQuery::InertiaTensor { .. } => "InertiaTensor",
            GeometryQuery::EdgeLength(_) => "EdgeLength",
            GeometryQuery::EdgeTangent(_) => "EdgeTangent",
            GeometryQuery::FaceNormal(_) => "FaceNormal",
            GeometryQuery::FaceSurfaceKind(_) => "FaceSurfaceKind",
            GeometryQuery::EdgeCurveKind(_) => "EdgeCurveKind",
            GeometryQuery::OwnerBody(_) => "OwnerBody",
            GeometryQuery::ClosestPointOnShape { .. } => "ClosestPointOnShape",
            GeometryQuery::PointOnShape { .. } => "PointOnShape",
            GeometryQuery::SurfaceAngle { .. } => "SurfaceAngle",
        }
    }
}

/// Per-query capability flag: which geometry representations a query can
/// operate on, as specified by PRD
/// `docs/prds/v0_3/kernel-geometry-queries.md` §5.4.
///
/// Used by the multi-kernel dispatcher to fail closed when a BRep-only query
/// is asked of a non-BRep (Mesh/Voxel/Sdf/VolumeMesh) realization. The gate
/// function that maps `(ReprKind, QueryCapability)` to a routing decision is
/// `reify_eval::geometry_ops::gate_query_capability`.
///
/// # Severity convention
///
/// | Variant       | Repr that satisfies it      |
/// |---------------|-----------------------------|
/// | `BRepOnly`    | [`ReprKind::BRep`] only     |
/// | `MeshOnly`    | [`ReprKind::Mesh`] only     |
/// | `BRepAndMesh` | Either [`ReprKind::BRep`] or [`ReprKind::Mesh`] |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueryCapability {
    /// Query can only be evaluated against a BRep (OCCT) representation.
    ///
    /// Examples from PRD §5.4: `edge_length`, `curvature` (KGQ-μ),
    /// `surface_curvature` (KGQ-μ), `perimeter` (KGQ-ν).
    BRepOnly,
    /// Query can only be evaluated against a Mesh (Manifold) representation.
    ///
    /// Reserved for future mesh-native queries; no extant variants as of v0.3.
    MeshOnly,
    /// Query can be evaluated against either BRep or Mesh representations.
    ///
    /// The dispatcher routes BRep inputs to OCCT and Mesh inputs to Manifold.
    BRepAndMesh,
}

impl GeometryQuery {
    /// Map each query variant to its capability class per PRD §5.4.
    ///
    /// The match is EXHAUSTIVE with NO `_` wildcard — mirroring the
    /// [`GeometryQuery::kind_name`] precedent. Adding a new `GeometryQuery`
    /// variant requires adding an arm here at the same diff site; the
    /// compiler enforces this, eliminating silent mis-routing of future
    /// BRep-only variants (e.g. `CurveCurvatureAt`/`SurfaceCurvatureAt`
    /// added by KGQ-μ, `Perimeter` by KGQ-ν) to the wrong kernel.
    ///
    /// # Adding new variants
    ///
    /// **BRep-only variants MUST add `=> QueryCapability::BRepOnly` here.**
    /// A wildcard `_ => BRepAndMesh` would silently mis-route future
    /// BRep-only queries to the Manifold kernel — the compiler enforces
    /// correctness at the diff site.
    // G-allow: task #3623 QueryCapability enum mapping; consumer is the capability-dispatch arm in subsequent #3623 steps
    pub fn capability_kind(&self) -> QueryCapability {
        match self {
            // §5.4 BRepOnly set (extant as of this commit; KGQ-μ adds
            // CurveCurvatureAt + SurfaceCurvatureAt; KGQ-ν adds Perimeter)
            GeometryQuery::EdgeLength(_) => QueryCapability::BRepOnly,

            // All other extant variants default to BRepAndMesh.
            GeometryQuery::Volume(_) => QueryCapability::BRepAndMesh,
            GeometryQuery::SurfaceArea(_) => QueryCapability::BRepAndMesh,
            GeometryQuery::Centroid(_) => QueryCapability::BRepAndMesh,
            GeometryQuery::BoundingBox(_) => QueryCapability::BRepAndMesh,
            GeometryQuery::Distance { .. } => QueryCapability::BRepAndMesh,
            GeometryQuery::MomentOfInertia { .. } => QueryCapability::BRepAndMesh,
            GeometryQuery::AdjacentFaces { .. } => QueryCapability::BRepAndMesh,
            GeometryQuery::SharedEdges { .. } => QueryCapability::BRepAndMesh,
            GeometryQuery::IsWatertight(_) => QueryCapability::BRepAndMesh,
            GeometryQuery::IsManifold(_) => QueryCapability::BRepAndMesh,
            GeometryQuery::IsOrientable(_) => QueryCapability::BRepAndMesh,
            GeometryQuery::CenterOfMass { .. } => QueryCapability::BRepAndMesh,
            GeometryQuery::InertiaTensor { .. } => QueryCapability::BRepAndMesh,
            GeometryQuery::EdgeTangent(_) => QueryCapability::BRepAndMesh,
            GeometryQuery::FaceNormal(_) => QueryCapability::BRepAndMesh,
            GeometryQuery::FaceSurfaceKind(_) => QueryCapability::BRepAndMesh,
            GeometryQuery::EdgeCurveKind(_) => QueryCapability::BRepAndMesh,
            GeometryQuery::AncestorFacesOfEdge { .. } => QueryCapability::BRepAndMesh,
            GeometryQuery::OwnerBody(_) => QueryCapability::BRepAndMesh,
            GeometryQuery::ClosestPointOnShape { .. } => QueryCapability::BRepAndMesh,
            GeometryQuery::PointOnShape { .. } => QueryCapability::BRepAndMesh,
            GeometryQuery::SurfaceAngle { .. } => QueryCapability::BRepAndMesh,
        }
    }
}

/// Geometric kind of a face's underlying surface, matching OCCT's
/// `GeomAbs_*` taxonomy via `BRepAdaptor_Surface::GetType()`.
///
/// Returned (or implied via the `Value::String` wire format) by
/// [`GeometryQuery::FaceSurfaceKind`]. Consumed by
/// `selector_vocabulary_v2::faces_by_surface_kind` to implement PRD
/// line 78's `%Plane`/`%Cylinder`/`%Cone`/`%Sphere`/`%Torus` slots.
///
/// The `BezierSurface`/`BSplineSurface` arms are kept distinct (rather
/// than collapsed under a generic `Spline`) because OCCT's classification
/// distinguishes them at the type level; the `OffsetSurface` arm is
/// preserved for completeness against the OCCT taxonomy. `Other` is the
/// safety-net arm for forward compatibility — a future OCCT version that
/// adds a new GeomAbs variant will surface as `Other` here, not silently
/// classify as one of the existing arms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FaceSurfaceKind {
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Torus,
    BezierSurface,
    BSplineSurface,
    OffsetSurface,
    Other,
}

impl FaceSurfaceKind {
    /// Decode a canonical surface-kind name into a [`FaceSurfaceKind`].
    ///
    /// Mirrors the wire format produced by
    /// [`GeometryQuery::FaceSurfaceKind`] and consumed by
    /// `selector_vocabulary_v2::faces_by_surface_kind`. Returns the
    /// originating string (so callers can embed it in error diagnostics)
    /// when the name is not one of the documented canonical strings.
    pub fn try_from_str(s: &str) -> Result<Self, &str> {
        match s {
            "Plane" => Ok(FaceSurfaceKind::Plane),
            "Cylinder" => Ok(FaceSurfaceKind::Cylinder),
            "Cone" => Ok(FaceSurfaceKind::Cone),
            "Sphere" => Ok(FaceSurfaceKind::Sphere),
            "Torus" => Ok(FaceSurfaceKind::Torus),
            "BezierSurface" => Ok(FaceSurfaceKind::BezierSurface),
            "BSplineSurface" => Ok(FaceSurfaceKind::BSplineSurface),
            "OffsetSurface" => Ok(FaceSurfaceKind::OffsetSurface),
            "Other" => Ok(FaceSurfaceKind::Other),
            _ => Err(s),
        }
    }
}

/// Geometric kind of an edge's underlying curve, matching OCCT's
/// `GeomAbs_*` taxonomy via `BRepAdaptor_Curve::GetType()`.
///
/// Returned (or implied via the `Value::String` wire format) by
/// [`GeometryQuery::EdgeCurveKind`]. Consumed by
/// `selector_vocabulary_v2::edges_by_curve_kind` to implement PRD line
/// 78's `%Line`/`%Circle`/`%Ellipse`/etc. slots.
///
/// `Other` is the safety-net arm for forward compatibility against new
/// OCCT GeomAbs variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeCurveKind {
    Line,
    Circle,
    Ellipse,
    Hyperbola,
    Parabola,
    BezierCurve,
    BSplineCurve,
    OffsetCurve,
    Other,
}

impl EdgeCurveKind {
    /// Decode a canonical curve-kind name into an [`EdgeCurveKind`].
    ///
    /// Mirrors the wire format produced by
    /// [`GeometryQuery::EdgeCurveKind`] and consumed by
    /// `selector_vocabulary_v2::edges_by_curve_kind`. Returns the
    /// originating string (so callers can embed it in error diagnostics)
    /// when the name is not one of the documented canonical strings.
    pub fn try_from_str(s: &str) -> Result<Self, &str> {
        match s {
            "Line" => Ok(EdgeCurveKind::Line),
            "Circle" => Ok(EdgeCurveKind::Circle),
            "Ellipse" => Ok(EdgeCurveKind::Ellipse),
            "Hyperbola" => Ok(EdgeCurveKind::Hyperbola),
            "Parabola" => Ok(EdgeCurveKind::Parabola),
            "BezierCurve" => Ok(EdgeCurveKind::BezierCurve),
            "BSplineCurve" => Ok(EdgeCurveKind::BSplineCurve),
            "OffsetCurve" => Ok(EdgeCurveKind::OffsetCurve),
            "Other" => Ok(EdgeCurveKind::Other),
            _ => Err(s),
        }
    }
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

/// FEA element-order discriminator for a tet-based [`VolumeMesh`].
///
/// `P1` tetrahedra carry 4 corner nodes per element (linear shape functions,
/// 4 indices in `tet_indices` per element). `P2` tetrahedra carry 10 nodes
/// per element — 4 corners plus 6 edge midpoints in Gmsh's canonical local
/// ordering (quadratic shape functions, 10 indices per element).
///
/// Used both as an explicit field on [`VolumeMesh`] and as one of the inputs
/// to the volume-mesh cache key (`reify_kernel_gmsh::cache_key`), so changing
/// element order between two otherwise-identical mesh requests produces a
/// distinct cache entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementOrderTag {
    /// 4-node tetrahedral element (linear shape functions).
    P1,
    /// 10-node tetrahedral element (quadratic shape functions; 4 corners +
    /// 6 edge midpoints in Gmsh canonical order).
    P2,
}

/// Volumetric tetrahedral mesh produced by the v0.3 surface→volume meshing
/// pipeline (e.g. Gmsh HXT).
///
/// Mirrors [`Mesh`]'s field shape (`vertices: Vec<f32>` flat XYZ triples,
/// optional flat `normals`) so existing helpers that walk vertex positions
/// can share code. The structural difference is `tet_indices`: a flat array
/// of **4 indices per element for P1** (one tet = 4 corner indices), or
/// **10 indices per element for P2** (4 corner + 6 edge-midpoint indices in
/// Gmsh's canonical local ordering). Distinct from `Mesh::indices` (3
/// indices per surface triangle).
///
/// `element_order` tags the per-element arity so downstream consumers
/// (`reify-solver-elastic` for FEA stiffness assembly, future GUI volume
/// renderers) can read the index stride without round-tripping through a
/// separate metadata channel.
#[derive(Debug, Clone)]
pub struct VolumeMesh {
    /// Vertex positions, flat [x0, y0, z0, x1, y1, z1, ...].
    pub vertices: Vec<f32>,
    /// Tet element indices: 4 per element for P1, 10 per element for P2
    /// (4 corner + 6 edge midpoints in Gmsh canonical order).
    pub tet_indices: Vec<u32>,
    /// Element order discriminator (P1 = 4 nodes/elem, P2 = 10 nodes/elem).
    pub element_order: ElementOrderTag,
    /// Optional per-vertex normals (flat, same layout as `vertices`); seldom
    /// populated for volume meshes since interior nodes have no canonical
    /// surface-normal direction, but kept here as an `Option` so a future
    /// boundary-extraction step can carry surface normals through without
    /// changing the type's shape.
    pub normals: Option<Vec<f32>>,
}

impl VolumeMesh {
    /// Read the XYZ position of node `idx` from the flat `vertices` buffer
    /// (layout: `[x0, y0, z0, x1, y1, z1, …]`, stride 3).
    ///
    /// Returns `None` if `idx * 3 + 3` would overflow `usize` or fall
    /// outside `vertices.len()`.  Callers map `None` to whatever
    /// crate-local error variant they prefer; e.g.
    /// `compute_dirichlet_bcs` in `reify-mesh-morph::boundary` maps it to
    /// `ProjectionFailure::InvalidNodeIndex(idx)`.
    ///
    /// The raw `f32` representation is returned; widening to `f64` for FEA
    /// arithmetic is the caller's responsibility.
    pub fn vertex(&self, idx: u32) -> Option<[f32; 3]> {
        let i = idx as usize;
        let base = i.checked_mul(3)?;
        let end = base.checked_add(3)?;
        if end > self.vertices.len() {
            return None;
        }
        Some([
            self.vertices[base],
            self.vertices[base + 1],
            self.vertices[base + 2],
        ])
    }

    /// Return the XYZ coordinates of node `idx` as `[f64; 3]`, widening from
    /// the stored `f32` representation.
    ///
    /// This is a f64-widening sibling of [`Self::vertex`], intended for
    /// callers whose downstream computation runs in f64 (FEA arithmetic,
    /// Laplacian smoothing).  All bounds-checking is delegated to `vertex`;
    /// this method adds no new indexing logic.
    ///
    /// Returns `None` for the same out-of-range or overflow conditions as
    /// `vertex`.
    pub fn vertex_f64(&self, idx: u32) -> Option<[f64; 3]> {
        self.vertex(idx)
            .map(|[x, y, z]| [x as f64, y as f64, z as f64])
    }
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
#[non_exhaustive]
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
#[non_exhaustive]
pub enum QueryError {
    InvalidHandle(GeometryHandleId),
    QueryFailed(String),
    /// A surface-differential query received a non-finite parametric input.
    ///
    /// Emitted by FFI guards (`OcctKernel::surface_normal_at`,
    /// `OcctKernel::curvature_at`) when `u` or `v` is NaN or ±Infinity.
    /// The variant echoes back the bad inputs so callers can surface
    /// structured diagnostics without parsing message strings.
    NonFiniteParameter {
        u: f64,
        v: f64,
    },
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryError::InvalidHandle(id) => write!(f, "invalid handle for query: {:?}", id),
            QueryError::QueryFailed(msg) => write!(f, "geometry query failed: {}", msg),
            QueryError::NonFiniteParameter { u, v } => write!(
                f,
                "geometry query received non-finite parameter: u={u}, v={v}"
            ),
        }
    }
}

impl std::error::Error for QueryError {}

/// Errors from constructing a [`BooleanOpParents`] value with mismatched
/// slice lengths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BooleanOpParentsError {
    /// The `faces` and `edges` slices passed to the constructor have different
    /// lengths. Each parent must appear in both slices at the same index.
    LengthMismatch { faces: usize, edges: usize },
}

impl fmt::Display for BooleanOpParentsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BooleanOpParentsError::LengthMismatch { faces, edges } => write!(
                f,
                "BooleanOpParents::NAry: faces.len() ({faces}) != edges.len() ({edges}); \
                 each parent must have an entry in both slices"
            ),
        }
    }
}

impl std::error::Error for BooleanOpParentsError {}

/// Per-op attribute-history record returned by
/// [`GeometryKernel::execute_with_history`].
///
/// Each variant carries the kernel-specific records needed by the
/// `reify_eval` propagation helpers to seed `TopologyAttributeTable`
/// entries for the result handles. `None` is the default for kernels
/// that do not override `execute_with_history` and for ops that do not
/// produce per-op attribute history (primitives, transforms, etc.).
///
/// Open for extension — task 5b adds `Sweep`/`Loft`, tasks 6-8 add
/// primitive/local/boolean variants. Consumers must pattern-match
/// exhaustively (no `Other(_)` escape hatch) so the dispatch site
/// surfaces missing variants at compile time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeHistory {
    /// No history available (default-impl path; non-attributable op).
    None,
    /// Records produced by `BRepPrimAPI_MakePrism` for `GeometryOp::Extrude`.
    Extrude(SweepOpHistoryRecords),
    /// Records produced by `BRepPrimAPI_MakeRevol` for `GeometryOp::Revolve`.
    Revolve(SweepOpHistoryRecords),
    /// Records produced by `BRepOffsetAPI_MakePipe` for `GeometryOp::Sweep`
    /// (single-parent profile-along-spine sweep; task 5b, #2619).
    Sweep(SweepOpHistoryRecords),
    /// Records produced by `BRepOffsetAPI_ThruSections` for
    /// `GeometryOp::Loft` (multi-parent profile-section loft; task 5b,
    /// #2619).
    Loft(LoftOpHistoryRecords),
}

/// Outcome of a [`KernelAttributeHook::propagate_attributes`] call (or of the
/// engine-side dispatcher [`propagate_via_kernel_attribute_hook`] in
/// `reify-eval`).
///
/// Three variants capture the semantically-distinct results the v0.2
/// persistent-naming-v2 PRD (docs/prds/v0_2/persistent-naming-v2.md line 70)
/// requires:
///
/// - [`KernelAttributeOutcome::Propagated`] — the hook ran and copied the
///   appropriate parent topology attributes onto the result handles. The
///   result table now contains the propagated entries.
/// - [`KernelAttributeOutcome::Discarded`] — the hook ran but **intentionally
///   did not preserve attributes** (e.g. heavy remeshing within tolerance, or
///   — in this v0.2 stub — deferred Manifold FFI). The hook itself emits the
///   `tracing::warn!` diagnostic before returning this variant; consumers do
///   NOT need to surface a duplicate diagnostic.
/// - [`KernelAttributeOutcome::FellThrough`] — the kernel does not advertise
///   a `KernelAttributeHook` at all (the trait default for
///   [`GeometryKernel::attribute_hook`] returned `None`). This is reserved for
///   the engine-side dispatcher; `KernelAttributeHook::propagate_attributes`
///   itself never returns `FellThrough` (its caller knows the hook ran). The
///   dispatcher emits a `tracing::debug!` diagnostic before returning this
///   variant — the no-hook case is informational, not a warning.
///
/// Splitting `Discarded` from `FellThrough` lets the consumer distinguish
/// "hook ran and gave up" from "no hook to run" without a separate accessor
/// call on the kernel.
///
/// No `Other(_)` escape hatch — consumers must pattern-match exhaustively so
/// future variants surface at compile time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelAttributeOutcome {
    /// The hook copied parent topology attributes onto the result handles.
    Propagated,
    /// The hook ran but intentionally lost attributes (with diagnostic).
    Discarded,
    /// No hook to run — kernel does not advertise a `KernelAttributeHook`.
    /// Returned only by the engine-side dispatcher, never by a hook impl.
    FellThrough,
}

/// Best-effort propagation of `TopologyAttribute`s through a non-OCCT kernel's
/// native operations.
///
/// Per `docs/prds/v0_2/persistent-naming-v2.md` line 70 ("Multi-kernel
/// propagation via `KernelAttributeHook` trait"), this trait lets kernels
/// whose native primitives expose a parent→child correspondence (e.g.
/// Manifold's `MeshGL` merge vectors + per-triangle `faceID` / `originalID`)
/// copy parent attributes onto result face handles after a Boolean op.
///
/// Kernels that have NO such correspondence (Fidget's SDF reps, OpenVDB's
/// voxel reps) deliberately do **not** implement this trait; they inherit
/// the [`GeometryKernel::attribute_hook`] default of `None`, and the
/// engine-side dispatcher [`propagate_via_kernel_attribute_hook`] in
/// `reify-eval` returns [`KernelAttributeOutcome::FellThrough`] for them so
/// selectors over those reps fall through to computed selectors.
///
/// `Send + Sync` matches [`GeometryKernel`] — the hook is held behind a
/// trait-object reference (`Option<&dyn KernelAttributeHook>`) returned by
/// `attribute_hook()`, and the engine may invoke it from the dispatcher
/// thread.
///
/// # Sibling helper
///
/// `reify-eval`'s
/// `reify_eval::propagate_attributes_via_brepalgoapi_history`
/// covers the BRep-side `BRepAlgoAPI_*` Modified/Generated/Deleted
/// propagation. The `KernelAttributeHook` trait is the analogue for non-BRep
/// kernels: the function signatures are deliberately analogous so that
/// reading either implementation makes the other intuitable.
pub trait KernelAttributeHook: Send + Sync {
    /// Best-effort attribute propagation across a kernel-native operation.
    ///
    /// Inputs:
    /// - `table` — the `TopologyAttributeTable` to update in place. Parent
    ///   entries are read; new entries are written for each result handle that
    ///   the kernel's native correspondence maps a parent attribute onto.
    /// - `op` — the `GeometryOp` whose result is being attributed. Used by the
    ///   hook to dispatch on the op kind (typically: Boolean Union/Difference/
    ///   Intersection on a mesh-Boolean kernel).
    /// - `parent_handles` — the parent solid handles passed to `op`, in the
    ///   order the op consumed them. Lookups against `table` use these handles.
    /// - `result_handle` — the kernel's freshly-allocated result handle. Hook
    ///   impls record entries against this handle (or against sub-handles
    ///   derived from it via the kernel's face/edge extraction; the trait does
    ///   not prescribe the exact sub-handle vocabulary because non-BRep kernels
    ///   may not use the same face/edge taxonomy as OCCT).
    /// - `splitting_feature_id` — the FeatureId whose op is being propagated,
    ///   for stamping into `ModEntry`s on splits (see `propagate_attributes_via_brepalgoapi_history` for the analogous use).
    ///
    /// Returns:
    /// - `Ok(KernelAttributeOutcome::Propagated)` if the hook copied the
    ///   parent attributes onto the result.
    /// - `Ok(KernelAttributeOutcome::Discarded)` if the hook intentionally did
    ///   not preserve attributes (heavy remeshing within tolerance, or — in
    ///   the current v0.2 Manifold stub — deferred FFI). The hook itself MUST
    ///   emit a `tracing::warn!` diagnostic before returning this variant.
    /// - `Err(QueryError)` for runtime kernel failures distinct from the
    ///   intentional Discarded path.
    ///
    /// Note: the hook impl never returns `KernelAttributeOutcome::FellThrough`
    /// — that variant is reserved for the engine-side dispatcher when no hook
    /// is advertised. Returning `FellThrough` from a hook impl would be a
    /// contract violation (and is structurally discouraged: the kernel-side
    /// caller already knows the hook ran).
    fn propagate_attributes(
        &self,
        table: &mut TopologyAttributeTable,
        op: &GeometryOp,
        parent_handles: &[GeometryHandleId],
        result_handle: GeometryHandleId,
        splitting_feature_id: &FeatureId,
    ) -> Result<KernelAttributeOutcome, QueryError>;
}

/// Trait for geometry kernels. Lives in reify-types for dependency inversion —
/// implemented in reify-kernel-occt, consumed by reify-eval via reify-geometry.
pub trait GeometryKernel: Send + Sync {
    /// Execute a geometry operation, returning a handle to the result.
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError>;

    /// Execute a geometry operation, returning the result handle paired with
    /// any per-op attribute-history records the kernel produces.
    ///
    /// The default implementation forwards to `execute(op)` and returns
    /// `AttributeHistory::None`, so non-overriding kernels (mocks,
    /// non-OCCT backends) compile and behave identically to today's
    /// `execute`-only path. Overriding kernels (e.g. `OcctKernelHandle`,
    /// task 5a) return `AttributeHistory::Extrude` /
    /// `AttributeHistory::Revolve` for ops where they have history records;
    /// the engine's dispatch site (`Engine::execute_realization_ops`)
    /// matches on the returned variant to seed `TopologyAttributeTable`
    /// entries.
    ///
    /// This is intentionally additive (rather than replacing `execute`) so
    /// the existing `execute(&op)` call sites can continue to use it
    /// without acquiring an unwanted `AttributeHistory` they would
    /// immediately discard.
    fn execute_with_history(
        &mut self,
        op: &GeometryOp,
    ) -> Result<(GeometryHandle, AttributeHistory), GeometryError> {
        let handle = self.execute(op)?;
        Ok((handle, AttributeHistory::None))
    }

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
    /// edge sub-shape (with `BRepKind::Edge`). The ordering follows the
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
    /// face sub-shape (with `BRepKind::Face`). The ordering follows the
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

    /// Extract the unique vertices of a shape, storing each as a new handle.
    ///
    /// Returns a `Vec<GeometryHandleId>` where each id names a freshly-stored
    /// vertex sub-shape (with `BRepKind::Vertex`). The ordering follows the
    /// kernel's canonical `TopExp::MapShapes(.., TopAbs_VERTEX, ..)` enumeration,
    /// deduplicated by `TopoDS_Shape::IsSame`.
    ///
    /// Default implementation returns
    /// `Err(QueryError::QueryFailed("topology extraction not supported by this kernel"))`,
    /// keeping non-OCCT kernels (mocks, stubs) compiling without per-impl edits.
    fn extract_vertices(
        &mut self,
        _handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        Err(QueryError::QueryFailed(
            "topology extraction not supported by this kernel".into(),
        ))
    }

    /// Ingest an externally-supplied [`Mesh`] and return a handle to the stored
    /// geometry.
    ///
    /// # Producer-side-only mesh ingest
    ///
    /// This method is the **structural enforcement** of "producer-side-only"
    /// mesh ingest: kernels whose geometry model is _not_ based on triangle
    /// meshes (Fidget's implicit SDF representations, OpenVDB's voxel grids,
    /// OCCT's B-rep topology, mocks, stubs) inherit this default unchanged, and
    /// the `Err(OperationFailed)` return is the observable contract for that
    /// absence.  The pattern is exactly analogous to [`attribute_hook`]'s
    /// `None` default (geometry.rs ~line 1735): the absence of an override IS
    /// the "not supported" contract — no per-kernel opt-out code is needed.
    ///
    /// `ManifoldKernel` is the only current override; it accepts closed
    /// orientable triangle meshes and stores them as `Manifold` values (see
    /// `crates/reify-kernel-manifold/src/kernel.rs`).
    ///
    /// # Object safety
    ///
    /// The trait remains object-safe — `Self` appears only in the `&mut self`
    /// receiver.  The `type_name::<Self>()` call lives in the method *body*;
    /// object safety is determined by the *signature* alone, so
    /// `Box<dyn GeometryKernel>` upcasts (e.g. `register.rs:58`,
    /// `kernel.rs:353`) keep compiling.  When invoked through a trait object,
    /// `Self = dyn GeometryKernel` and `type_name` yields `"dyn GeometryKernel"`
    /// (acceptable for the not-supported path); when called on a concrete kernel
    /// directly, `type_name` yields the concrete kernel's fully-qualified name.
    ///
    /// # This is intentionally additive
    ///
    /// Following the established pattern at
    /// [`GeometryKernel::execute_with_history`] (default
    /// `AttributeHistory::None`) and [`GeometryKernel::attribute_hook`] (default
    /// `None`), this default allows all existing kernels to continue compiling
    /// without per-impl changes.
    fn ingest_mesh(&mut self, _mesh: &Mesh) -> Result<GeometryHandle, GeometryError> {
        Err(GeometryError::OperationFailed(format!(
            "{} does not accept Mesh inputs",
            std::any::type_name::<Self>()
        )))
    }

    /// Optional best-effort `TopologyAttribute` propagation hook for non-OCCT
    /// kernels with native parent→child correspondence (e.g. Manifold's
    /// `MeshGL` merge vectors + per-triangle `faceID` / `originalID`).
    ///
    /// Returns `None` by default — kernels without a native attribute-tracking
    /// channel (Fidget's SDF reps, OpenVDB's voxel reps, mocks, stubs) inherit
    /// this default and selectors over their reps fall through to computed
    /// selectors via [`KernelAttributeOutcome::FellThrough`] in the engine-side
    /// dispatcher (`reify-eval::propagate_via_kernel_attribute_hook`).
    ///
    /// Per `docs/prds/v0_2/persistent-naming-v2.md` line 70, this default is
    /// the **structural enforcement** of "Fidget/OpenVDB don't implement the
    /// trait — selectors fall through to computed selectors": no per-kernel
    /// opt-out code is needed in fidget/openvdb because the absence of an
    /// override IS the fall-through contract.
    ///
    /// Manifold's adapter (`reify-kernel-manifold`) overrides this to return
    /// `Some(self)`, providing the first concrete impl of
    /// [`KernelAttributeHook`].
    ///
    /// This is intentionally additive (rather than required), following the
    /// established pattern at [`GeometryKernel::execute_with_history`]
    /// (default `AttributeHistory::None`).
    fn attribute_hook(&self) -> Option<&dyn KernelAttributeHook> {
        None
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

    /// Derive the `FeatureId` for the mid-surface of `parent`.
    ///
    /// Returns `<parent>/mid_surface` (composed via the [`fmt::Display`]
    /// impl), e.g. `Bracket#realization[0]` → `Bracket#realization[0]/mid_surface`.
    /// Composition is well-defined: nesting yields `<parent>/mid_surface/mid_surface`.
    ///
    /// Implements the derived-geometry naming sub-vocabulary from PRD
    /// `docs/prds/v0_4/structural-analysis-shells.md` line 81 (T20), built
    /// on top of the path-based feature identity established by PRD
    /// `docs/prds/v0_2/persistent-naming-v2.md` line 33.
    pub fn derived_mid_surface(parent: &FeatureId) -> FeatureId {
        FeatureId::new(format!("{parent}/mid_surface"))
    }
}

impl fmt::Display for FeatureId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&reify_core::identity::RealizationNodeId> for FeatureId {
    fn from(id: &reify_core::identity::RealizationNodeId) -> Self {
        Self(id.to_string())
    }
}

/// Per-axis sign discriminator for box-primitive corner vertices.
///
/// `Pos` selects the positive face along an axis, `Neg` the negative face.
/// Used for all three axes in `Role::CornerVertex { x, y, z }` to uniquely
/// name each of a box's 8 corners as a sign triple.
/// PRD `docs/prds/v0_3/mesh-morphing-phase-2.md` §3.1 (task α).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AxisSign {
    Pos,
    Neg,
}

/// Cap orientation for the `Role::Cap` variant.
///
/// Two semantic flavours of cap exist:
///   - `Top` / `Bottom`: gravitational orientation, used by extrude
///     (where `Top` is the swept-end face / `LastShape()` and `Bottom`
///     is the profile-as-placed / `FirstShape()`).
///   - `Start` / `End`: parametric sequence along a sweep parameter,
///     used by revolve (where `Start` is the profile at angle 0 /
///     `FirstShape()` and `End` is the profile at the angle endpoint /
///     `LastShape()`). For full-2π revolutions both caps collapse and
///     no `Cap` entries are emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CapKind {
    Top,
    Bottom,
    Start,
    End,
}

/// One entry in a topology entity's mod-history postfix.
///
/// Recorded each time a feature splits a parent topology entity into
/// children — `splitting_feature_id` is the FeatureId whose op caused
/// the split, and `split_index` distinguishes the resulting children
/// (PRD lines 60, 64).
///
/// Populated by `reify_eval::propagate_attributes_via_brepalgoapi_history`
/// when a parent topology entity is split into multiple result sub-shapes
/// (count > 1 across same-kind Modified ∪ Generated). Tasks 5/7/8 add
/// per-op coverage for sweeps/local-features/booleans.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModEntry {
    pub splitting_feature_id: FeatureId,
    pub split_index: u32,
}

/// Role of a topology entity within its originating feature.
///
/// The minimal initial set per PRD line 56. Tasks 5-8 (sweeps, primitives,
/// local features, booleans) will add per-op variants here as a closed
/// extension — there is intentionally no `Other(String)` escape hatch so
/// that selector-resolution exhaustive matching remains auditable.
///
/// Per-op vocabulary:
///   - **Extrude** (`GeometryOp::Extrude`): `Cap(Top)` / `Cap(Bottom)` for
///     end faces, `Side` for lateral faces, `NewEdge` for cap-to-side edges.
///   - **Revolve** (`GeometryOp::Revolve`): `Cap(Start)` / `Cap(End)` for
///     profile faces of partial revolutions (omitted for full-2π),
///     `RevolvedFace` for lateral revolved faces, `AxisFace` reserved for
///     faces that touch the revolve axis (declared but not yet detected
///     by `populate_revolve_attributes`; see task 5a design decisions).
///   - **Sweep** (`GeometryOp::Sweep`): `Cap(Start)` / `Cap(End)` for
///     profile-as-placed and swept-end caps (single profile follows a
///     spine; parametric sequence along the spine), `SweptFace` for
///     lateral faces generated by sweeping a profile edge along the
///     spine (sweep convention; distinct from `Side` and `RevolvedFace`
///     so selectors can match per-op).
///   - **Loft** (`GeometryOp::Loft`): `Cap(Start)` / `Cap(End)` for the
///     first / last profile-section caps under `is_solid=true` (closed
///     ends of the multi-section solid), `LoftedFace` for lateral faces
///     generated between consecutive section profiles (loft convention;
///     distinct from `Side`, `RevolvedFace`, and `SweptFace` so selectors
///     can match per-op).
///   - **Mid-surface (derived geometry)**: `MidSurfaceFace` for per-region
///     mid-surface patches (`local_index = region.label`), `MidSurfaceEdge`
///     for inter-region adjacency curves (`local_index` = canonical sort
///     position of the `(min, max)` region pair). Emitted by
///     `reify_shell_extract::populate_mid_surface_attributes`; PRD
///     `docs/prds/v0_4/structural-analysis-shells.md` line 81 (T20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// A cap face (`top`/`bottom` for extrude, `start`/`end` for revolve)
    /// of the feature.
    Cap(CapKind),
    /// A lateral side face of the feature (extrude convention).
    Side,
    /// An edge created by the feature's construction (e.g. cap-to-side
    /// boundary edges of an extrude).
    NewEdge,
    /// A revolved lateral face — a face generated by sweeping a profile
    /// edge around the revolve axis (revolve convention; distinct from
    /// `Side` so selectors can match per-op).
    RevolvedFace,
    /// A face that touches the revolve axis (e.g. zero-area axis-coincident
    /// face, or a face whose surface contains the axis). Reserved for
    /// detection in a follow-up task; not currently emitted by
    /// `populate_revolve_attributes` but declared here so the variant
    /// space is stable for selector vocabulary v2 (PRD line 102).
    AxisFace,
    /// A swept lateral face — a face generated by sweeping a profile
    /// edge along the sweep spine (sweep convention; distinct from
    /// `Side` and `RevolvedFace` so selectors can match per-op).
    /// Emitted by `populate_sweep_attributes` for `GeometryOp::Sweep`
    /// (task 5b, #2619).
    SweptFace,
    /// A lofted lateral face — a face generated between consecutive
    /// profile sections of a loft (loft convention; distinct from
    /// `Side`, `RevolvedFace`, and `SweptFace` so selectors can match
    /// per-op). Emitted by `populate_loft_attributes` for
    /// `GeometryOp::Loft` (task 5b, #2619).
    LoftedFace,
    /// A mid-surface patch corresponding to one segmentation region of
    /// a body's derived mid-surface (derived-geometry naming, PRD
    /// `docs/prds/v0_4/structural-analysis-shells.md` line 81).
    /// `local_index = region.label` (BFS-discovery order from
    /// `reify_shell_extract::segmentation`). Emitted by
    /// `reify_shell_extract::populate_mid_surface_attributes`.
    MidSurfaceFace,
    /// An inter-region adjacency edge of a body's derived mid-surface.
    /// `local_index` is the canonical sort position of the `(min, max)`
    /// region pair (ascending tuple order). Emitted by
    /// `reify_shell_extract::populate_mid_surface_attributes` from PRD
    /// `docs/prds/v0_4/structural-analysis-shells.md` line 81 (T20).
    MidSurfaceEdge,
    /// A corner vertex of a box primitive — uniquely identified by the
    /// three face-signs (±X, ±Y, ±Z) that meet at it. Produces 8 distinct
    /// values per box. Emitted by per-primitive vertex seeders (task C).
    ///
    /// PRD `docs/prds/v0_3/mesh-morphing-phase-2.md` §3.1 (task α).
    CornerVertex {
        x: AxisSign,
        y: AxisSign,
        z: AxisSign,
    },
    /// A corner vertex of a swept solid where a cap face meets the lateral
    /// envelope. Emitted by per-op vertex seeders for extrude / revolve /
    /// sweep / loft (task C). `face` records which cap (top/bottom for
    /// gravitational sweeps; start/end for parametric sweeps).
    ///
    /// PRD `docs/prds/v0_3/mesh-morphing-phase-2.md` §3.1 (task α).
    CapCornerVertex { face: CapKind },
}

/// Per-topology-entity attribute record for v0.2 persistent naming.
///
/// One of these is associated with each face/edge produced by a feature,
/// keyed by `GeometryHandleId` in the runtime `TopologyAttributeTable`.
///
/// Fields per PRD lines 52-61:
///   - `feature_id` — the feature that produced (or last touched) this entity.
///   - `role` — what part of the feature this entity is.
///   - `local_index` — 0-based index within `(feature_id, role)`. Tasks 5-8
///     populate this from per-op routing.
///   - `user_label` — optional user-supplied name (absorbs v0.1 `name = "..."`
///     syntax, PRD line 50). `None` is the common default.
///   - `mod_history` — lineage postfix populated on splits by task 3 (#2571).
///
/// Note: deliberately not `Hash` — `Vec<ModEntry>` would force a Hash bound
/// chain, and TopologyAttribute is never used as a HashMap key (the table
/// is keyed by GeometryHandleId).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopologyAttribute {
    pub feature_id: FeatureId,
    pub role: Role,
    pub local_index: u32,
    pub user_label: Option<String>,
    pub mod_history: Vec<ModEntry>,
}

impl TopologyAttribute {
    /// Returns `true` iff the parent-key fields (`feature_id`, `role`,
    /// `local_index`, `user_label`) match.
    ///
    /// The `mod_history` field is intentionally excluded — split children
    /// of the same parent share the parent-key but differ in `mod_history`;
    /// this predicate detects that clustering. Used by:
    ///   - `reify_eval::topology_attribute_resolver` to route multi-match
    ///     resolutions to `AmbiguousAfterSplit` rather than `Unresolved`
    ///     when the matched set all shares one parent.
    ///   - Future task-10 `split_by(...)` selector and task-4 local_index
    ///     reassignment diagnostic, which will reuse the same predicate.
    ///
    /// See PRD docs/prds/v0_2/persistent-naming-v2.md line 64
    /// (modification-history postfix).
    pub fn same_parent_as(&self, other: &Self) -> bool {
        self.feature_id == other.feature_id
            && self.role == other.role
            && self.local_index == other.local_index
            && self.user_label == other.user_label
    }
}

/// Runtime table mapping geometry handle ids to `TopologyAttribute`s.
///
/// The v0.2 attribute-based replacement-in-progress for `FeatureTagTable`.
/// Tasks 5-8 wire per-op auto-population; task 2 (#2570) wires
/// selector lookup against this table. Mirrors the `FeatureTagTable`
/// shape (HashMap keyed by `GeometryHandleId`, four-method API) so the
/// existing call sites can adopt it incrementally.
#[derive(Debug, Default)]
pub struct TopologyAttributeTable {
    entries: HashMap<GeometryHandleId, TopologyAttribute>,
}

impl TopologyAttributeTable {
    /// Record that geometry handle `id` carries `attr`.
    ///
    /// Overwrites any prior entry for the same id (last-write-wins,
    /// mirroring `FeatureTagTable::record`). Tasks 3 (#2571) and 4 (#2572)
    /// will add diagnostics around accidental rebinds.
    pub fn record(&mut self, id: GeometryHandleId, attr: TopologyAttribute) {
        self.entries.insert(id, attr);
    }

    /// Look up the attribute for a given geometry handle, if any.
    pub fn lookup(&self, id: GeometryHandleId) -> Option<&TopologyAttribute> {
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

    /// Iterate over all `(GeometryHandleId, &TopologyAttribute)` pairs in the table.
    ///
    /// Iteration order is **unspecified** — the table is HashMap-backed, so
    /// callers needing a deterministic order must collect and sort
    /// (e.g. by `GeometryHandleId` or `(feature_id, role, local_index)`).
    ///
    /// Used by per-realization fragility detection in
    /// `reify_eval::engine_build` to filter the just-completed realization's
    /// attribute entries (`attr.feature_id == realization_feature_id`) for
    /// the `detect_local_index_reassignment_diagnostics` helper
    /// (PRD `docs/prds/v0_2/persistent-naming-v2.md` line 72).
    pub fn iter(&self) -> impl Iterator<Item = (GeometryHandleId, &TopologyAttribute)> {
        self.entries.iter().map(|(k, v)| (*k, v))
    }
}

// --- BRepAlgoAPI history records (v0.2 persistent-naming-v2, task 2590) ---
//
// These records describe the parent-to-child mapping produced by a constructive
// boolean operation (currently `BRepAlgoAPI_Fuse`; Cut/Common in task 8). The
// records are pure data — they do not depend on any kernel-specific type — and
// live in `reify-types` rather than `reify-kernel-occt` so that consumers
// (notably `reify_eval::propagate_attributes_via_brepalgoapi_history`) can
// reference them without taking a normal-dep on `reify-kernel-occt`. Pulling
// `reify-kernel-occt` into `reify-eval`'s normal compile graph would
// transitively drag it into every workspace member that dev-depends on
// `reify-test-support` (via its `eval-helpers` feature), defeating the OCCT
// gating defined in `scripts/occt-touching-crates.txt`.

/// One BRepAlgoAPI Modified or Generated record: a parent sub-shape (face
/// or edge) at index `parent_subshape_index` of parent `parent_index`
/// gives rise to a result sub-shape at index `result_subshape_index` in
/// the fused result. All indices are 0-based and follow the canonical
/// `TopExp::MapShapes(.., TopAbs_FACE | TopAbs_EDGE, ..)` order
/// (deduplicated by `IsSame`).
///
/// `parent_index` is `0` for the left operand, `1` for the right operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryRecord {
    pub parent_index: u8,
    pub parent_subshape_index: u32,
    pub result_subshape_index: u32,
}

/// One BRepAlgoAPI Deleted record: a parent sub-shape at
/// `parent_subshape_index` of parent `parent_index` was consumed by the
/// boolean operation and has no analogue in the result.
///
/// `parent_index` is `0` for the left operand, `1` for the right operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeletedRecord {
    pub parent_index: u8,
    pub parent_subshape_index: u32,
}

/// All BRepAlgoAPI history records for a single boolean operation,
/// split by sub-shape kind (face / edge) and by record kind
/// (Modified / Generated / Deleted).
///
/// Returned by `OcctKernel::boolean_fuse_with_history` and
/// `OcctKernelHandle::boolean_fuse_with_history`. Consumed by
/// `reify_eval::propagate_attributes_via_brepalgoapi_history` to copy
/// parent topology attributes onto the result handles after a
/// constructive boolean.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BooleanOpHistoryRecords {
    /// Number of Modified/Generated children that the FFI primitive observed
    /// but could not map back into the result face/edge map (i.e. the child
    /// shape reported by BRepAlgoAPI was absent from the result's TopExp map).
    /// For vanilla boolean operations this should be zero; a non-zero value
    /// indicates a kernel correspondence loss or map-type mismatch.
    ///
    /// **Bulk counter:** this is a single accumulator that aggregates drops
    /// across all four `emit_history_for_parent` invocations in
    /// `boolean_fuse_with_history` — that is, (left faces) + (right faces) +
    /// (left edges) + (right edges). It does **not** break down by shape kind
    /// (face vs. edge) or by which operand (left vs. right) produced the miss.
    ///
    /// **Diagnostic note:** the increment path inside `emit_history_for_parent`
    /// (C++ wrapper) is only tested indirectly through the zero-count assertion
    /// in the canonical happy-path integration test. A dedicated test exercising
    /// the non-zero path (e.g. a stub result map missing one child) is deferred
    /// to a follow-up task.
    ///
    /// **TODO:** wire this counter into error reporting so that a non-zero count
    /// surfaces as a warning log or `QueryError::QueryFailed` from
    /// `propagate_attributes_via_brepalgoapi_history`, rather than being silently
    /// recorded. Until that follow-up lands, callers must inspect this field
    /// manually if they need to detect kernel correspondence loss. If the wiring
    /// task requires actionable per-kind or per-operand diagnostics, split
    /// `BooleanOpHistory.silent_drop_count` (C++ struct) into separate face/edge
    /// or left/right counters before adding new consumers; the deferred split is
    /// intentional pending that task's specification of required granularity.
    pub silent_drop_count: u32,
    pub face_modified: Vec<HistoryRecord>,
    pub face_generated: Vec<HistoryRecord>,
    pub face_deleted: Vec<DeletedRecord>,
    pub edge_modified: Vec<HistoryRecord>,
    pub edge_generated: Vec<HistoryRecord>,
    pub edge_deleted: Vec<DeletedRecord>,
}

/// All Modified / Generated / Deleted history records for a single
/// **single-parent sweep operation** (extrude / revolve, currently;
/// sweep / loft in task 5b).
///
/// Mirrors `BooleanOpHistoryRecords` but for ops with one parent profile
/// instead of two operands; `parent_index` on the inner records is
/// always `0` and is included only for layout-uniformity with the
/// boolean variant. The two extra fields `start_cap_face_indices` and
/// `end_cap_face_indices` capture the cap-face information that is
/// **not** exposed via Modified/Generated maps but is available via
/// `BRepBuilderAPI_Sweep::FirstShape()` and `LastShape()`.
///
/// Cap orientation conventions (per `populate_extrude_attributes` /
/// `populate_revolve_attributes`):
///   - For extrude: `start_cap_face_indices` → `Cap(Top)` (the
///     swept-end face / `LastShape()`-derived; chosen so a positive-Z
///     prism's "top" face matches gravitational orientation),
///     `end_cap_face_indices` → `Cap(Bottom)`.
///   - For revolve: `start_cap_face_indices` → `Cap(Start)` (profile at
///     angle 0, FirstShape), `end_cap_face_indices` → `Cap(End)`
///     (profile at the angle endpoint, LastShape). Both lists are empty
///     for full-2π revolutions.
///
/// Returned by `OcctKernel::extrude_with_history` and
/// `OcctKernel::revolve_with_history` (task 5a). Consumed by
/// `reify_eval::populate_extrude_attributes` and
/// `populate_revolve_attributes` to seed `TopologyAttributeTable`
/// entries on the result handles.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SweepOpHistoryRecords {
    /// Count of Modified/Generated children that the FFI sweep primitive
    /// observed but could not map back into the result face/edge map (i.e.
    /// the child shape reported by BRepPrimAPI / BRepOffsetAPI was absent
    /// from the result's TopExp map). For vanilla sweep operations this
    /// should be zero; a non-zero value indicates a kernel correspondence
    /// loss or map-type mismatch.
    ///
    /// **Bulk counter:** this is a single accumulator that aggregates drops
    /// across all four `emit_sweep_*` invocations per sweep variant — that
    /// is, `make_prism_with_history` / `make_revolve_with_history` /
    /// `make_pipe_with_history` each accumulate drops from their respective
    /// Modified/Generated/face/edge emission calls into this field. It does
    /// **not** break down by shape kind (face vs. edge) or by which operand
    /// produced the miss.
    ///
    /// **Diagnostic note:** the increment path inside the C++ sweep helpers
    /// is only tested indirectly through the zero-count assertion in the
    /// canonical happy-path integration tests. A dedicated test exercising
    /// the non-zero path (e.g. a synthetic input that triggers the
    /// `result_map.FindIndex(child) < 1` branch) is a deferred follow-up;
    /// tracked in project memory under
    /// `"SweepOpHistory silent_drop_count non-zero path test"`.
    ///
    /// **TODO (follow-up — tracked in project memory "SweepOpHistory
    /// silent_drop_count error reporting"):** wire this counter into error
    /// reporting so that a non-zero count surfaces as a warning log, rather
    /// than being silently recorded. Until that follow-up lands, callers
    /// must inspect this field manually if they need to detect kernel
    /// correspondence loss. If the wiring task requires actionable per-kind
    /// diagnostics, split `SweepOpHistory.silent_drop_count` (C++ struct)
    /// into separate face/edge counters before adding new consumers; the
    /// deferred split is intentional pending that task's specification of
    /// required granularity.
    pub silent_drop_count: u32,
    pub face_modified: Vec<HistoryRecord>,
    pub face_generated: Vec<HistoryRecord>,
    pub face_deleted: Vec<DeletedRecord>,
    pub edge_modified: Vec<HistoryRecord>,
    pub edge_generated: Vec<HistoryRecord>,
    pub edge_deleted: Vec<DeletedRecord>,
    /// Result-face indices (into the result shape's TopExp face map)
    /// that correspond to the profile-as-placed cap (extrude bottom /
    /// revolve start).
    pub start_cap_face_indices: Vec<u32>,
    /// Result-face indices (into the result shape's TopExp face map)
    /// that correspond to the swept-end cap (extrude top / revolve
    /// end). Empty for full-2π revolutions where the start and end
    /// profile coincide and no cap face exists.
    pub end_cap_face_indices: Vec<u32>,
    /// Count of non-degenerate, untracked profile edges that passed through
    /// `synthesize_full_revolution_radial_face_records` without producing a
    /// `face_generated` record. Covers the axial-classifier path (path 4:
    /// `dot(edge_dir, axis) > 1 − DIR_TOL`), the slanted-classifier path
    /// (path 5: `dot > DIR_TOL`), and the inner face-matching fall-through
    /// (path 6: no candidate face passes the normal-parallel + axial-coord
    /// checks). Degenerate edges are NOT counted. Always 0 for prism ops and
    /// partial revolves; non-zero indicates a synthesis gap in a full revolve.
    ///
    /// **Trace emission:** when non-zero, the C++ synthesis helper emits one
    /// `Message_Warning` via `Message::DefaultMessenger()` summarising the
    /// total count (one aggregate message per call, not per edge, so log
    /// volume is bounded regardless of how many edges are missing).
    ///
    /// **Integration test guard:** integration tests for well-formed profiles
    /// should assert `unsynthesized_profile_edge_count == 0`. The self-consistency
    /// test `full_revolve_misclassified_radial_edge_counter_best_effort`
    /// validates the increment path via a synthetic near-axial edge
    /// (`dot ≈ 2e-6`, just over `DIR_TOL = 1e-6`), using the assertion
    /// `unsynthesized_profile_edge_count == n_profile_edges − face_generated.len()`
    /// so the test is agnostic to whether OCCT covers the edge independently.
    pub unsynthesized_profile_edge_count: u32,
    /// Count of `face_generated` records dropped by the post-sort dedup pass
    /// because their `parent_subshape_index` duplicated the immediately
    /// preceding record (after stable-sort by `parent_subshape_index`).
    /// Always 0 for well-formed profiles; non-zero indicates OCCT emitted a
    /// duplicate edge report or a synthesized record collided with an
    /// OCCT-reported one (the `tracked_parent_edges` guard prevents the
    /// latter for fully-radial edges but not for partial reports).
    ///
    /// **Drop policy:** the first occurrence under stable sort is kept;
    /// subsequent duplicates are dropped so surviving records are strictly
    /// increasing in `parent_subshape_index`. This preserves the
    /// `local_index = parent_subshape_index` invariant that
    /// `populate_revolve_attributes` relies on. Debug builds additionally
    /// `assert()` the post-dedup vector is strictly increasing, giving a
    /// loud signal during local development without crashing release builds.
    ///
    /// **Integration test guard:** assert `duplicate_parent_subshape_index_count
    /// == 0` in happy-path full-revolve tests. The FFI fixture
    /// `revolve_synthesis_post_sort_for_test` enables white-box testing of
    /// the dedup logic on synthetic flat inputs without real OCCT geometry.
    pub duplicate_parent_subshape_index_count: u32,
}

/// All Modified / Generated / Deleted history records for a
/// **multi-parent loft operation** (`GeometryOp::Loft`).
///
/// Mirrors `SweepOpHistoryRecords`'s field layout but **without** the
/// diagnostic counters `unsynthesized_profile_edge_count` and
/// `duplicate_parent_subshape_index_count` — those are revolve-synthesis-
/// specific (task 2706) and have no analogue in loft. Loft has no
/// full-revolution synthesis post-pass, and loft's per-section emission
/// order (sequential `local_index` across all sections) naturally avoids
/// the duplicate-index pathology that motivated the dedup pass for
/// revolve.
///
/// `parent_index` semantics differ from `SweepOpHistoryRecords`: rather
/// than always being `0` (single profile), it denotes the **section
/// index** in `[0, N)` where `N = profiles.len()` of the
/// `GeometryOp::Loft { profiles }` op. `parent_subshape_index` is the
/// edge index within that section's edge map.
///
/// Cap orientation conventions:
///   - `start_cap_face_indices` → `Cap(Start)` (first profile section,
///     `BRepOffsetAPI_ThruSections::FirstShape()`-derived under
///     `is_solid=true`).
///   - `end_cap_face_indices` → `Cap(End)` (last profile section,
///     `LastShape()`-derived under `is_solid=true`).
///
/// Both lists are empty when the underlying loft is constructed with
/// `is_solid=false` (open-shell loft) — though task 5b's caller hard-
/// codes `is_solid=true` to match `GeometryOp::Loft`'s contract today.
///
/// Returned by `OcctKernel::loft_with_history` (task 5b). Consumed by
/// `reify_eval::populate_loft_attributes` to seed
/// `TopologyAttributeTable` entries on the result handles.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoftOpHistoryRecords {
    pub face_modified: Vec<HistoryRecord>,
    pub face_generated: Vec<HistoryRecord>,
    pub face_deleted: Vec<DeletedRecord>,
    pub edge_modified: Vec<HistoryRecord>,
    pub edge_generated: Vec<HistoryRecord>,
    pub edge_deleted: Vec<DeletedRecord>,
    /// Result-face indices (into the result shape's TopExp face map)
    /// that correspond to the first profile-section cap (loft start
    /// under `is_solid=true`).
    pub start_cap_face_indices: Vec<u32>,
    /// Result-face indices (into the result shape's TopExp face map)
    /// that correspond to the last profile-section cap (loft end
    /// under `is_solid=true`).
    pub end_cap_face_indices: Vec<u32>,
}

/// Typed wrapper for the per-parent face/edge handle slices passed to
/// [`reify_eval::propagate_attributes_via_brepalgoapi_history`].
///
/// Introduced in v0.2 persistent-naming-v2 (task 2590 / PRD §6.5) to
/// replace the raw `&[&[GeometryHandleId]]` slice-of-slices parameters
/// and make the binary-fuse parent-index semantics explicit at the
/// call site.
///
/// ## Variant semantics
///
/// - **`Binary`** — exactly two parents, `faces[0]` / `edges[0]` is the
///   left operand and `faces[1]` / `edges[1]` is the right operand,
///   matching `HistoryRecord::parent_index` (`0` = left, `1` = right per
///   the doc on [`HistoryRecord`]).  Use this for `BRepAlgoAPI_Fuse`,
///   `BRepAlgoAPI_Cut`, and `BRepAlgoAPI_Common`.
///
/// - **`NAry`** — arbitrary number of parents for multi-input fuse
///   (`BRepAlgoAPI_BuilderAlgo`). `faces[i]` / `edges[i]` correspond to
///   parent `i` (i.e. `HistoryRecord::parent_index == i`). The two inner
///   slices must have the same length; this is a caller invariant — the
///   propagation function surfaces `QueryFailed` for any out-of-bounds
///   index.
///
/// The accessor methods [`face_slices`][Self::face_slices] and
/// [`edge_slices`][Self::edge_slices] return a unified
/// `&[&'a [GeometryHandleId]]` view regardless of variant, so the inner
/// propagation helper works on raw indices without variant awareness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOpParents<'a> {
    /// Binary boolean (fuse / cut / common): exactly two parents.
    /// `faces[0]` / `edges[0]` = left operand;
    /// `faces[1]` / `edges[1]` = right operand.
    Binary {
        faces: [&'a [GeometryHandleId]; 2],
        edges: [&'a [GeometryHandleId]; 2],
    },
    /// N-ary boolean (multi-input fuse): arbitrary number of parents.
    /// `faces[i]` / `edges[i]` correspond to `HistoryRecord::parent_index == i`.
    ///
    /// **Invariant:** `faces.len() == edges.len()`. Use
    /// [`BooleanOpParents::try_nary`] (fallible) or
    /// [`BooleanOpParents::nary`] (panicking) to construct checked instances.
    /// Direct enum-literal construction (`BooleanOpParents::NAry { … }`) is
    /// still permitted but is **unchecked** — the caller is responsible for
    /// ensuring the two slices have the same length.
    NAry {
        faces: &'a [&'a [GeometryHandleId]],
        edges: &'a [&'a [GeometryHandleId]],
    },
}

/// Debug-build invariant check shared by `BooleanOpParents::NAry` accessors.
/// Panics in debug builds (no-op in release) when `faces.len() != edges.len()`,
/// using `BooleanOpParentsError::LengthMismatch`'s Display impl as the
/// canonical wording. Module-private: the only callers are the in-module
/// `face_slices` / `edge_slices` accessors.
fn debug_check_nary_invariant(faces: &[&[GeometryHandleId]], edges: &[&[GeometryHandleId]]) {
    debug_assert!(
        faces.len() == edges.len(),
        "{}",
        BooleanOpParentsError::LengthMismatch {
            faces: faces.len(),
            edges: edges.len(),
        },
    );
}

impl<'a> BooleanOpParents<'a> {
    /// Checked constructor for the [`NAry`][Self::NAry] variant.
    ///
    /// Returns `Ok(Self::NAry { faces, edges })` when `faces.len() ==
    /// edges.len()`, otherwise `Err(BooleanOpParentsError::LengthMismatch)`.
    /// Prefer this over direct enum-literal construction when the slice
    /// lengths are not statically guaranteed to match.
    pub fn try_nary(
        faces: &'a [&'a [GeometryHandleId]],
        edges: &'a [&'a [GeometryHandleId]],
    ) -> Result<Self, BooleanOpParentsError> {
        if faces.len() != edges.len() {
            Err(BooleanOpParentsError::LengthMismatch {
                faces: faces.len(),
                edges: edges.len(),
            })
        } else {
            Ok(Self::NAry { faces, edges })
        }
    }

    /// Panicking constructor for the [`NAry`][Self::NAry] variant.
    ///
    /// Equivalent to `Self::try_nary(faces, edges).unwrap_or_else(|e| panic!("{e}"))`.
    /// Use this at call sites where a length mismatch is a programmer bug;
    /// use [`try_nary`][Self::try_nary] where a mismatch is a recoverable error.
    ///
    /// # Panics
    ///
    /// Panics with a message containing `"faces.len()"` when
    /// `faces.len() != edges.len()`.
    pub fn nary(faces: &'a [&'a [GeometryHandleId]], edges: &'a [&'a [GeometryHandleId]]) -> Self {
        Self::try_nary(faces, edges).unwrap_or_else(|e| panic!("{e}"))
    }

    /// Returns the per-parent face-handle slices as a flat slice of slices,
    /// regardless of variant. Index `i` gives the face handles for parent `i`.
    ///
    /// For [`NAry`][Self::NAry] instances, length correctness is the caller's
    /// responsibility when using direct enum-literal construction. Use
    /// [`try_nary`][Self::try_nary] or [`nary`][Self::nary] to obtain a
    /// checked instance. A `debug_assert!` fires in debug builds if a
    /// direct-literal construction is called with mismatched lengths.
    pub fn face_slices(&self) -> &[&'a [GeometryHandleId]] {
        match self {
            Self::Binary { faces, .. } => &faces[..],
            Self::NAry { faces, edges } => {
                debug_check_nary_invariant(faces, edges);
                faces
            }
        }
    }

    /// Returns the per-parent edge-handle slices as a flat slice of slices,
    /// regardless of variant. Index `i` gives the edge handles for parent `i`.
    ///
    /// For [`NAry`][Self::NAry] instances, length correctness is the caller's
    /// responsibility when using direct enum-literal construction. Use
    /// [`try_nary`][Self::try_nary] or [`nary`][Self::nary] to obtain a
    /// checked instance. A `debug_assert!` fires in debug builds if a
    /// direct-literal construction is called with mismatched lengths.
    pub fn edge_slices(&self) -> &[&'a [GeometryHandleId]] {
        match self {
            Self::Binary { edges, .. } => &edges[..],
            Self::NAry { faces, edges } => {
                debug_check_nary_invariant(faces, edges);
                edges
            }
        }
    }
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
            x1: 0.0,
            y1: 0.0,
            z1: 0.0,
            x2: 1.0,
            y2: 2.0,
            z2: 3.0,
        };
        match &op {
            GeometryOp::LineSegment {
                x1,
                y1,
                z1,
                x2,
                y2,
                z2,
            } => {
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
            GeometryOp::Arc {
                center,
                radius,
                start_angle,
                end_angle,
                axis,
            } => {
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
            GeometryOp::Helix {
                radius,
                pitch,
                height,
            } => {
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
            points: vec![
                [0.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [2.0, 0.0, 0.0],
                [3.0, 1.0, 0.0],
            ],
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
            control_points: vec![
                [0.0, 0.0, 0.0],
                [1.0, 2.0, 0.0],
                [3.0, 2.0, 0.0],
                [4.0, 0.0, 0.0],
            ],
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
            control_points: vec![
                [0.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [2.0, 0.0, 0.0],
                [3.0, 1.0, 0.0],
            ],
            weights: vec![1.0, 1.0, 1.0, 1.0],
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            degree: 3,
        };
        match &op {
            GeometryOp::NurbsCurve {
                control_points,
                weights,
                knots,
                degree,
            } => {
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
            fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
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
        assert_eq!(
            result.len(),
            2,
            "query_many should return one Value per query"
        );
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
            fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
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
    fn b_rep_kind_face_and_edge_variants_exist() {
        // Construct and pattern-match the BRepKind::Edge variant.
        let edge_repr = BRepKind::Edge;
        match edge_repr {
            BRepKind::Edge => {}
            other => panic!("expected BRepKind::Edge, got {:?}", other),
        }

        // Construct and pattern-match the BRepKind::Face variant.
        let face_repr = BRepKind::Face;
        match face_repr {
            BRepKind::Face => {}
            other => panic!("expected BRepKind::Face, got {:?}", other),
        }

        // Edge and Face must be distinguishable from each other and from
        // existing variants (Wire/Shell/Solid/Compound).
        assert_ne!(BRepKind::Edge, BRepKind::Face);
        assert_ne!(BRepKind::Edge, BRepKind::Wire);
        assert_ne!(BRepKind::Face, BRepKind::Shell);
        assert_ne!(BRepKind::Edge, BRepKind::Solid);
        assert_ne!(BRepKind::Face, BRepKind::Compound);

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
    fn brep_kind_vertex_variant_is_pattern_matchable_and_distinct() {
        let v = BRepKind::Vertex;
        match v {
            BRepKind::Vertex => {}
            other => panic!("expected BRepKind::Vertex, got {:?}", other),
        }

        // Vertex must be distinguishable from all other BRepKind variants.
        assert_ne!(BRepKind::Vertex, BRepKind::Edge);
        assert_ne!(BRepKind::Vertex, BRepKind::Face);
        assert_ne!(BRepKind::Vertex, BRepKind::Solid);
        assert_ne!(BRepKind::Vertex, BRepKind::Shell);
        assert_ne!(BRepKind::Vertex, BRepKind::Wire);
        assert_ne!(BRepKind::Vertex, BRepKind::Compound);
    }

    #[test]
    fn closest_point_on_shape_variant_is_constructible_and_matchable() {
        // Pin the shape of the new ClosestPointOnShape variant — the eval-side
        // dispatcher (task 2324) builds and reads back exactly these fields.
        let cp = GeometryQuery::ClosestPointOnShape {
            handle: GeometryHandleId(17),
            px: 1.0,
            py: 2.0,
            pz: 3.0,
        };
        match &cp {
            GeometryQuery::ClosestPointOnShape { handle, px, py, pz } => {
                assert_eq!(*handle, GeometryHandleId(17));
                assert_eq!(*px, 1.0);
                assert_eq!(*py, 2.0);
                assert_eq!(*pz, 3.0);
            }
            _ => panic!("expected ClosestPointOnShape variant"),
        }
    }

    #[test]
    fn point_on_shape_variant_is_constructible_and_matchable() {
        // Pin the shape of the new PointOnShape variant — the dispatcher
        // supplies tolerance from `DEFAULT_POINT_ON_SHAPE_TOLERANCE_M`
        // (= OCCT `Precision::Confusion()`, ~1e-7) by default.
        let pos = GeometryQuery::PointOnShape {
            handle: GeometryHandleId(19),
            px: 4.0,
            py: 5.0,
            pz: 6.0,
            tolerance: super::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
        };
        match &pos {
            GeometryQuery::PointOnShape {
                handle,
                px,
                py,
                pz,
                tolerance,
            } => {
                assert_eq!(*handle, GeometryHandleId(19));
                assert_eq!(*px, 4.0);
                assert_eq!(*py, 5.0);
                assert_eq!(*pz, 6.0);
                assert_eq!(*tolerance, super::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M);
            }
            _ => panic!("expected PointOnShape variant"),
        }
    }

    #[test]
    fn surface_angle_variant_is_constructible_and_matchable() {
        // Pin the shape of the new SurfaceAngle variant — kernel returns
        // the unsigned angle in radians ∈ [0, π].
        let sa = GeometryQuery::SurfaceAngle {
            face_a: GeometryHandleId(23),
            face_b: GeometryHandleId(29),
        };
        match &sa {
            GeometryQuery::SurfaceAngle { face_a, face_b } => {
                assert_eq!(*face_a, GeometryHandleId(23));
                assert_eq!(*face_b, GeometryHandleId(29));
            }
            _ => panic!("expected SurfaceAngle variant"),
        }
    }

    #[test]
    fn debug_assert_query_many_invariant_passes_when_lengths_match() {
        // Empty batch: the boundary case most likely to expose an off-by-one
        // bug if the helper's comparison were inverted.
        debug_assert_query_many_invariant(&[] as &[GeometryQuery], &[] as &[Value]);

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
        use reify_core::identity::RealizationNodeId;
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

    // --- task 3033 (T20): derived-geometry naming sub-vocabulary ---
    // PRD `docs/prds/v0_4/structural-analysis-shells.md` line 81 pins the
    // derived-geometry naming form `<parent>/mid_surface`.  This test
    // covers both the single-step derivation and the nested case (the
    // function must compose via Display rather than a one-off suffix).

    #[test]
    fn feature_id_derived_mid_surface_returns_parent_path_with_mid_surface_suffix() {
        let parent = FeatureId::new("Bracket#realization[0]");
        assert_eq!(
            FeatureId::derived_mid_surface(&parent),
            FeatureId::new("Bracket#realization[0]/mid_surface")
        );
        // Nested derivation must compose via Display, not a single-shot
        // suffix; this pins the implementation to `format!("{parent}/mid_surface")`.
        let nested = FeatureId::derived_mid_surface(&FeatureId::derived_mid_surface(&parent));
        assert_eq!(
            nested,
            FeatureId::new("Bracket#realization[0]/mid_surface/mid_surface")
        );
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
    #[allow(clippy::clone_on_copy)] // intentional: exercises Clone impl on a Copy type
    fn role_clone_preserves_identity() {
        let r = Role::Cap(CapKind::Bottom);
        let s = r;
        assert_eq!(r, s);
        let copy = r.clone();
        assert_eq!(r, copy);
    }

    // --- task 5a (#2573): new Role + CapKind variants for revolve ---

    #[test]
    fn cap_kind_start_and_end_are_distinct() {
        assert_ne!(CapKind::Start, CapKind::End);
    }

    #[test]
    fn cap_kind_start_and_end_differ_from_top_and_bottom() {
        // Per design decision: Top/Bottom is gravitational orientation
        // (extrude convention); Start/End is parametric sequence (revolve
        // angle convention). All four must be pairwise distinct.
        assert_ne!(CapKind::Start, CapKind::Top);
        assert_ne!(CapKind::Start, CapKind::Bottom);
        assert_ne!(CapKind::End, CapKind::Top);
        assert_ne!(CapKind::End, CapKind::Bottom);
    }

    #[test]
    fn cap_kind_start_and_end_debug_format() {
        let dbg_start = format!("{:?}", CapKind::Start);
        let dbg_end = format!("{:?}", CapKind::End);
        assert!(dbg_start.contains("Start"), "expected Start in {dbg_start}");
        assert!(dbg_end.contains("End"), "expected End in {dbg_end}");
    }

    #[test]
    #[allow(clippy::clone_on_copy)] // intentional: exercises Clone impl on a Copy type
    fn cap_kind_start_and_end_clone_round_trips() {
        let s = CapKind::Start;
        let s_copy = s;
        assert_eq!(s, s_copy);
        let s_clone = s.clone();
        assert_eq!(s, s_clone);

        let e = CapKind::End;
        let e_copy = e;
        assert_eq!(e, e_copy);
        let e_clone = e.clone();
        assert_eq!(e, e_clone);
    }

    #[test]
    fn role_revolved_face_and_axis_face_are_distinct() {
        assert_ne!(Role::RevolvedFace, Role::AxisFace);
    }

    #[test]
    fn role_revolved_face_differs_from_side_and_caps() {
        // RevolvedFace is the per-op distinguisher for revolve lateral faces;
        // it must not collide with Side (extrude lateral) or any Cap variant.
        assert_ne!(Role::RevolvedFace, Role::Side);
        assert_ne!(Role::RevolvedFace, Role::NewEdge);
        assert_ne!(Role::RevolvedFace, Role::Cap(CapKind::Top));
        assert_ne!(Role::RevolvedFace, Role::Cap(CapKind::Bottom));
        assert_ne!(Role::RevolvedFace, Role::Cap(CapKind::Start));
        assert_ne!(Role::RevolvedFace, Role::Cap(CapKind::End));
    }

    #[test]
    fn role_axis_face_differs_from_side_and_caps() {
        // AxisFace is reserved for axis-touching faces (revolve only); it
        // must be distinct from every other variant including RevolvedFace.
        assert_ne!(Role::AxisFace, Role::Side);
        assert_ne!(Role::AxisFace, Role::NewEdge);
        assert_ne!(Role::AxisFace, Role::Cap(CapKind::Top));
        assert_ne!(Role::AxisFace, Role::Cap(CapKind::Bottom));
        assert_ne!(Role::AxisFace, Role::Cap(CapKind::Start));
        assert_ne!(Role::AxisFace, Role::Cap(CapKind::End));
    }

    #[test]
    fn role_cap_start_and_cap_end_distinct_from_existing_caps() {
        assert_ne!(Role::Cap(CapKind::Start), Role::Cap(CapKind::End));
        assert_ne!(Role::Cap(CapKind::Start), Role::Cap(CapKind::Top));
        assert_ne!(Role::Cap(CapKind::End), Role::Cap(CapKind::Bottom));
    }

    #[test]
    fn role_revolved_face_and_axis_face_debug_format() {
        let dbg_rf = format!("{:?}", Role::RevolvedFace);
        let dbg_af = format!("{:?}", Role::AxisFace);
        assert!(
            dbg_rf.contains("RevolvedFace"),
            "expected RevolvedFace in {dbg_rf}"
        );
        assert!(dbg_af.contains("AxisFace"), "expected AxisFace in {dbg_af}");
    }

    #[test]
    fn role_corner_vertex_distinguishes_all_eight_box_corners() {
        use std::collections::HashSet;
        let mut set: HashSet<Role> = HashSet::new();
        for x in [AxisSign::Pos, AxisSign::Neg] {
            for y in [AxisSign::Pos, AxisSign::Neg] {
                for z in [AxisSign::Pos, AxisSign::Neg] {
                    set.insert(Role::CornerVertex { x, y, z });
                }
            }
        }
        assert_eq!(
            set.len(),
            8,
            "8 sign-combo corners must be distinct Role values"
        );

        // Distinct from CapCornerVertex
        assert_ne!(
            Role::CornerVertex {
                x: AxisSign::Pos,
                y: AxisSign::Pos,
                z: AxisSign::Pos
            },
            Role::CapCornerVertex { face: CapKind::Top },
        );
        // Distinct from pre-existing Role variants
        assert_ne!(
            Role::CornerVertex {
                x: AxisSign::Pos,
                y: AxisSign::Pos,
                z: AxisSign::Pos
            },
            Role::Side,
        );
        assert_ne!(
            Role::CornerVertex {
                x: AxisSign::Pos,
                y: AxisSign::Pos,
                z: AxisSign::Pos
            },
            Role::NewEdge,
        );
        assert_ne!(
            Role::CornerVertex {
                x: AxisSign::Pos,
                y: AxisSign::Pos,
                z: AxisSign::Pos
            },
            Role::RevolvedFace,
        );
    }

    #[test]
    fn role_cap_corner_vertex_distinguishes_all_four_cap_faces() {
        use std::collections::HashSet;
        let cap_kinds = [CapKind::Top, CapKind::Bottom, CapKind::Start, CapKind::End];
        let set: HashSet<Role> = cap_kinds
            .iter()
            .map(|f| Role::CapCornerVertex { face: *f })
            .collect();
        assert_eq!(
            set.len(),
            4,
            "4 CapKind variants must yield 4 distinct CapCornerVertex roles"
        );

        // Distinct from CornerVertex
        assert_ne!(
            Role::CapCornerVertex { face: CapKind::Top },
            Role::CornerVertex {
                x: AxisSign::Pos,
                y: AxisSign::Pos,
                z: AxisSign::Pos
            },
        );
        // Distinct from pre-existing Role variants
        assert_ne!(Role::CapCornerVertex { face: CapKind::Top }, Role::Side);
        assert_ne!(Role::CapCornerVertex { face: CapKind::Top }, Role::NewEdge);
        assert_ne!(
            Role::CapCornerVertex { face: CapKind::Top },
            Role::RevolvedFace
        );
    }

    #[test]
    #[allow(clippy::clone_on_copy)] // intentional: exercises Clone impl on a Copy type
    fn role_revolved_face_and_axis_face_clone_round_trips() {
        let r = Role::RevolvedFace;
        let r_copy = r;
        assert_eq!(r, r_copy);
        let r_clone = r.clone();
        assert_eq!(r, r_clone);

        let a = Role::AxisFace;
        let a_copy = a;
        assert_eq!(a, a_copy);
        let a_clone = a.clone();
        assert_eq!(a, a_clone);
    }

    // --- task 5a (#2573): SweepOpHistoryRecords (single-parent sweep ops) ---

    #[test]
    fn sweep_op_history_records_default_is_empty() {
        let records = SweepOpHistoryRecords::default();
        assert!(records.face_modified.is_empty());
        assert!(records.face_generated.is_empty());
        assert!(records.face_deleted.is_empty());
        assert!(records.edge_modified.is_empty());
        assert!(records.edge_generated.is_empty());
        assert!(records.edge_deleted.is_empty());
        assert!(records.start_cap_face_indices.is_empty());
        assert!(records.end_cap_face_indices.is_empty());
    }

    #[test]
    fn sweep_op_history_records_construct_with_all_vec_fields() {
        // Smoke-test that every field is a populated `Vec<...>` of the expected
        // record type. Mirrors the BooleanOpHistoryRecords field shape but with
        // explicit cap-index lists for caps (Modified/Generated alone do not
        // identify which faces are caps; the cap lists come from
        // BRepBuilderAPI_Sweep::FirstShape()/LastShape()).
        let records = SweepOpHistoryRecords {
            silent_drop_count: 0,
            face_modified: vec![HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 1,
                result_subshape_index: 2,
            }],
            face_generated: vec![HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 0,
                result_subshape_index: 7,
            }],
            face_deleted: vec![DeletedRecord {
                parent_index: 0,
                parent_subshape_index: 9,
            }],
            edge_modified: vec![HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 3,
                result_subshape_index: 4,
            }],
            edge_generated: vec![HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 5,
                result_subshape_index: 6,
            }],
            edge_deleted: vec![DeletedRecord {
                parent_index: 0,
                parent_subshape_index: 8,
            }],
            start_cap_face_indices: vec![5, 6],
            end_cap_face_indices: vec![7],
            unsynthesized_profile_edge_count: 0,
            duplicate_parent_subshape_index_count: 0,
        };
        assert_eq!(records.face_modified.len(), 1);
        assert_eq!(records.face_generated.len(), 1);
        assert_eq!(records.face_deleted.len(), 1);
        assert_eq!(records.edge_modified.len(), 1);
        assert_eq!(records.edge_generated.len(), 1);
        assert_eq!(records.edge_deleted.len(), 1);
        assert_eq!(records.start_cap_face_indices, vec![5_u32, 6]);
        assert_eq!(records.end_cap_face_indices, vec![7_u32]);
    }

    #[test]
    fn sweep_op_history_records_clone_preserves_value() {
        let records = SweepOpHistoryRecords {
            silent_drop_count: 0,
            face_modified: Vec::new(),
            face_generated: vec![HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 0,
                result_subshape_index: 7,
            }],
            face_deleted: Vec::new(),
            edge_modified: Vec::new(),
            edge_generated: Vec::new(),
            edge_deleted: Vec::new(),
            start_cap_face_indices: vec![5],
            end_cap_face_indices: vec![6],
            unsynthesized_profile_edge_count: 0,
            duplicate_parent_subshape_index_count: 0,
        };
        let cloned = records.clone();
        assert_eq!(records, cloned);
        assert_eq!(cloned.start_cap_face_indices, vec![5_u32]);
        assert_eq!(cloned.end_cap_face_indices, vec![6_u32]);
    }

    #[test]
    fn sweep_op_history_records_full_revolution_has_empty_cap_lists() {
        // For a full-2π revolve, FirstShape() and LastShape() reference the
        // same closed surface; the FFI layer leaves both cap lists empty in
        // that case. The record type allows expressing this directly.
        let records = SweepOpHistoryRecords {
            face_generated: vec![HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 0,
                result_subshape_index: 0,
            }],
            ..SweepOpHistoryRecords::default()
        };
        assert!(records.start_cap_face_indices.is_empty());
        assert!(records.end_cap_face_indices.is_empty());
        assert_eq!(records.face_generated.len(), 1);
    }

    // --- task 5a (#2573): AttributeHistory enum + execute_with_history default ---

    #[test]
    fn attribute_history_variants_construct_and_match() {
        // None — used by non-OCCT kernels and non-attributable ops.
        let none = AttributeHistory::None;
        match &none {
            AttributeHistory::None => {}
            _ => panic!("expected AttributeHistory::None"),
        }

        // Extrude — wraps SweepOpHistoryRecords.
        let extrude = AttributeHistory::Extrude(SweepOpHistoryRecords {
            start_cap_face_indices: vec![5],
            end_cap_face_indices: vec![6],
            ..SweepOpHistoryRecords::default()
        });
        match &extrude {
            AttributeHistory::Extrude(records) => {
                assert_eq!(records.start_cap_face_indices, vec![5_u32]);
                assert_eq!(records.end_cap_face_indices, vec![6_u32]);
            }
            _ => panic!("expected AttributeHistory::Extrude"),
        }

        // Revolve — same shape as Extrude but a distinct variant so the
        // dispatch site can route per-op.
        let revolve = AttributeHistory::Revolve(SweepOpHistoryRecords::default());
        match &revolve {
            AttributeHistory::Revolve(records) => {
                assert!(records.start_cap_face_indices.is_empty());
                assert!(records.end_cap_face_indices.is_empty());
            }
            _ => panic!("expected AttributeHistory::Revolve"),
        }
    }

    #[test]
    fn attribute_history_variants_are_distinct_via_partial_eq() {
        let none = AttributeHistory::None;
        let extrude = AttributeHistory::Extrude(SweepOpHistoryRecords::default());
        let revolve = AttributeHistory::Revolve(SweepOpHistoryRecords::default());

        assert_ne!(none, extrude);
        assert_ne!(none, revolve);
        // Same SweepOpHistoryRecords payload, different enum tag → !=.
        assert_ne!(extrude, revolve);
    }

    #[test]
    fn attribute_history_clone_round_trips() {
        let extrude = AttributeHistory::Extrude(SweepOpHistoryRecords {
            start_cap_face_indices: vec![1, 2],
            end_cap_face_indices: vec![3],
            ..SweepOpHistoryRecords::default()
        });
        let cloned = extrude.clone();
        assert_eq!(extrude, cloned);
    }

    #[test]
    fn attribute_history_debug_includes_variant_name() {
        let dbg = format!("{:?}", AttributeHistory::None);
        assert!(dbg.contains("None"), "expected None in {dbg}");
        let dbg = format!(
            "{:?}",
            AttributeHistory::Extrude(SweepOpHistoryRecords::default())
        );
        assert!(dbg.contains("Extrude"), "expected Extrude in {dbg}");
        let dbg = format!(
            "{:?}",
            AttributeHistory::Revolve(SweepOpHistoryRecords::default())
        );
        assert!(dbg.contains("Revolve"), "expected Revolve in {dbg}");
    }

    #[test]
    fn geometry_kernel_execute_with_history_default_returns_none_history() {
        // Verify that the default `execute_with_history` impl on `GeometryKernel`
        // forwards to `execute(op)?` and pairs the resulting handle with
        // `AttributeHistory::None`. Non-OCCT/non-overriding kernels and
        // non-attributable ops must route through this default unchanged.
        struct ExecuteOnlyKernel {
            next_id: u64,
        }

        impl GeometryKernel for ExecuteOnlyKernel {
            fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
                let id = self.next_id;
                self.next_id += 1;
                Ok(GeometryHandle {
                    id: GeometryHandleId(id),
                    repr: Some(BRepKind::Solid),
                })
            }

            fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
                unimplemented!("ExecuteOnlyKernel only supports execute")
            }

            fn export(
                &self,
                _handle: GeometryHandleId,
                _format: ExportFormat,
                _writer: &mut dyn std::io::Write,
            ) -> Result<(), ExportError> {
                unimplemented!()
            }

            fn tessellate(
                &self,
                _handle: GeometryHandleId,
                _tolerance: f64,
            ) -> Result<Mesh, TessError> {
                unimplemented!()
            }
        }

        let mut kernel = ExecuteOnlyKernel { next_id: 1 };

        // Non-attributable op — expect None.
        let op = GeometryOp::Sphere {
            radius: Value::Real(1.0),
        };
        let (handle, history) = kernel
            .execute_with_history(&op)
            .expect("execute_with_history default must succeed when execute does");
        assert_eq!(handle.id, GeometryHandleId(1));
        assert_eq!(history, AttributeHistory::None);

        // Attributable op (Extrude) — default impl still returns None because
        // the kernel does not override execute_with_history. Overriding kernels
        // (OcctKernelHandle, step-8/10) supply the Extrude/Revolve variant.
        let op = GeometryOp::Extrude {
            profile: GeometryHandleId(99),
            distance: Value::Real(5.0),
        };
        let (handle, history) = kernel
            .execute_with_history(&op)
            .expect("default impl must succeed for any GeometryOp execute supports");
        assert_eq!(handle.id, GeometryHandleId(2));
        assert_eq!(history, AttributeHistory::None);

        // Same for Revolve.
        let op = GeometryOp::Revolve {
            profile: GeometryHandleId(99),
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            angle_rad: std::f64::consts::PI,
        };
        let (handle, history) = kernel.execute_with_history(&op).unwrap();
        assert_eq!(handle.id, GeometryHandleId(3));
        assert_eq!(history, AttributeHistory::None);
    }

    #[test]
    fn geometry_kernel_execute_with_history_default_propagates_execute_error() {
        struct AlwaysFailKernel;

        impl GeometryKernel for AlwaysFailKernel {
            fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
                Err(GeometryError::OperationFailed("simulated".into()))
            }

            fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
                unimplemented!()
            }

            fn export(
                &self,
                _handle: GeometryHandleId,
                _format: ExportFormat,
                _writer: &mut dyn std::io::Write,
            ) -> Result<(), ExportError> {
                unimplemented!()
            }

            fn tessellate(
                &self,
                _handle: GeometryHandleId,
                _tolerance: f64,
            ) -> Result<Mesh, TessError> {
                unimplemented!()
            }
        }

        let mut kernel = AlwaysFailKernel;
        let op = GeometryOp::Sphere {
            radius: Value::Real(1.0),
        };
        let err = kernel
            .execute_with_history(&op)
            .expect_err("execute_with_history must propagate execute errors");
        match err {
            GeometryError::OperationFailed(msg) => assert!(msg.contains("simulated")),
            other => panic!("expected OperationFailed, got {:?}", other),
        }
    }

    // --- task 5b (#2619): SweptFace + LoftedFace Role variants ---

    #[test]
    fn role_swept_face_and_lofted_face_are_distinct() {
        assert_ne!(Role::SweptFace, Role::LoftedFace);
    }

    #[test]
    fn role_swept_face_differs_from_existing_variants() {
        // SweptFace is the per-op distinguisher for `GeometryOp::Sweep`
        // lateral faces; it must not collide with Side (extrude lateral),
        // RevolvedFace (revolve lateral), AxisFace, NewEdge, or any Cap.
        assert_ne!(Role::SweptFace, Role::Side);
        assert_ne!(Role::SweptFace, Role::NewEdge);
        assert_ne!(Role::SweptFace, Role::RevolvedFace);
        assert_ne!(Role::SweptFace, Role::AxisFace);
        assert_ne!(Role::SweptFace, Role::Cap(CapKind::Top));
        assert_ne!(Role::SweptFace, Role::Cap(CapKind::Bottom));
        assert_ne!(Role::SweptFace, Role::Cap(CapKind::Start));
        assert_ne!(Role::SweptFace, Role::Cap(CapKind::End));
    }

    #[test]
    fn role_lofted_face_differs_from_existing_variants() {
        // LoftedFace is the per-op distinguisher for `GeometryOp::Loft`
        // lateral faces; it must be distinct from every other variant.
        assert_ne!(Role::LoftedFace, Role::Side);
        assert_ne!(Role::LoftedFace, Role::NewEdge);
        assert_ne!(Role::LoftedFace, Role::RevolvedFace);
        assert_ne!(Role::LoftedFace, Role::AxisFace);
        assert_ne!(Role::LoftedFace, Role::Cap(CapKind::Top));
        assert_ne!(Role::LoftedFace, Role::Cap(CapKind::Bottom));
        assert_ne!(Role::LoftedFace, Role::Cap(CapKind::Start));
        assert_ne!(Role::LoftedFace, Role::Cap(CapKind::End));
    }

    #[test]
    fn role_swept_face_and_lofted_face_debug_format() {
        let dbg_sf = format!("{:?}", Role::SweptFace);
        let dbg_lf = format!("{:?}", Role::LoftedFace);
        assert!(
            dbg_sf.contains("SweptFace"),
            "expected SweptFace in {dbg_sf}"
        );
        assert!(
            dbg_lf.contains("LoftedFace"),
            "expected LoftedFace in {dbg_lf}"
        );
    }

    #[test]
    #[allow(clippy::clone_on_copy)] // intentional: exercises Clone impl on a Copy type
    fn role_swept_face_and_lofted_face_clone_round_trips() {
        let s = Role::SweptFace;
        let s_copy = s;
        assert_eq!(s, s_copy);
        let s_clone = s.clone();
        assert_eq!(s, s_clone);

        let l = Role::LoftedFace;
        let l_copy = l;
        assert_eq!(l, l_copy);
        let l_clone = l.clone();
        assert_eq!(l, l_clone);
    }

    #[test]
    fn role_swept_face_and_lofted_face_are_hash() {
        // Hash bound is required because TopologyAttributeTable keys are
        // GeometryHandleId but selector resolvers can group by role; assert
        // by exercising HashSet membership.
        use std::collections::HashSet;
        let mut roles: HashSet<Role> = HashSet::new();
        roles.insert(Role::SweptFace);
        roles.insert(Role::LoftedFace);
        assert!(roles.contains(&Role::SweptFace));
        assert!(roles.contains(&Role::LoftedFace));
    }

    // --- task 3033 (T20): MidSurfaceFace + MidSurfaceEdge Role variants ---

    #[test]
    fn role_mid_surface_face_and_edge_variants_are_distinct_from_each_other_and_existing_variants()
    {
        // Pairwise distinctness: the two new variants must differ from
        // each other AND from every existing Role variant. Mirrors the
        // discipline of the SweptFace/LoftedFace distinctness suite.
        // (Hash/Copy/Debug bounds are guaranteed by the `#[derive(...)]`
        // attribute on `Role` and don't need behavioral assertions here.)
        assert_ne!(Role::MidSurfaceFace, Role::MidSurfaceEdge);
        for existing in [
            Role::Cap(CapKind::Top),
            Role::Cap(CapKind::Bottom),
            Role::Cap(CapKind::Start),
            Role::Cap(CapKind::End),
            Role::Side,
            Role::NewEdge,
            Role::RevolvedFace,
            Role::AxisFace,
            Role::SweptFace,
            Role::LoftedFace,
        ] {
            assert_ne!(Role::MidSurfaceFace, existing);
            assert_ne!(Role::MidSurfaceEdge, existing);
        }
    }

    // --- task 5b (#2619): LoftOpHistoryRecords (multi-parent loft op) ---

    #[test]
    fn loft_op_history_records_default_is_empty() {
        let records = LoftOpHistoryRecords::default();
        assert!(records.face_modified.is_empty());
        assert!(records.face_generated.is_empty());
        assert!(records.face_deleted.is_empty());
        assert!(records.edge_modified.is_empty());
        assert!(records.edge_generated.is_empty());
        assert!(records.edge_deleted.is_empty());
        assert!(records.start_cap_face_indices.is_empty());
        assert!(records.end_cap_face_indices.is_empty());
    }

    #[test]
    fn loft_op_history_records_construct_with_all_vec_fields() {
        // For loft, parent_index = section index (0..N-1 across N profiles).
        // Field shape mirrors SweepOpHistoryRecords without the diagnostic
        // counters (those are revolve-synthesis-specific).
        let records = LoftOpHistoryRecords {
            face_modified: vec![HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 1,
                result_subshape_index: 2,
            }],
            face_generated: vec![
                HistoryRecord {
                    parent_index: 0,
                    parent_subshape_index: 0,
                    result_subshape_index: 4,
                },
                HistoryRecord {
                    parent_index: 1,
                    parent_subshape_index: 0,
                    result_subshape_index: 5,
                },
            ],
            face_deleted: vec![DeletedRecord {
                parent_index: 0,
                parent_subshape_index: 9,
            }],
            edge_modified: vec![HistoryRecord {
                parent_index: 1,
                parent_subshape_index: 3,
                result_subshape_index: 4,
            }],
            edge_generated: vec![HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 5,
                result_subshape_index: 6,
            }],
            edge_deleted: vec![DeletedRecord {
                parent_index: 1,
                parent_subshape_index: 8,
            }],
            start_cap_face_indices: vec![5, 6],
            end_cap_face_indices: vec![7],
        };
        assert_eq!(records.face_modified.len(), 1);
        assert_eq!(records.face_generated.len(), 2);
        assert_eq!(records.face_deleted.len(), 1);
        assert_eq!(records.edge_modified.len(), 1);
        assert_eq!(records.edge_generated.len(), 1);
        assert_eq!(records.edge_deleted.len(), 1);
        assert_eq!(records.start_cap_face_indices, vec![5_u32, 6]);
        assert_eq!(records.end_cap_face_indices, vec![7_u32]);
        // Confirm parent_index distinguishes sections.
        assert_eq!(records.face_generated[0].parent_index, 0);
        assert_eq!(records.face_generated[1].parent_index, 1);
    }

    #[test]
    fn loft_op_history_records_clone_preserves_value() {
        let records = LoftOpHistoryRecords {
            face_modified: Vec::new(),
            face_generated: vec![HistoryRecord {
                parent_index: 1,
                parent_subshape_index: 0,
                result_subshape_index: 7,
            }],
            face_deleted: Vec::new(),
            edge_modified: Vec::new(),
            edge_generated: Vec::new(),
            edge_deleted: Vec::new(),
            start_cap_face_indices: vec![5],
            end_cap_face_indices: vec![6],
        };
        let cloned = records.clone();
        assert_eq!(records, cloned);
        assert_eq!(cloned.start_cap_face_indices, vec![5_u32]);
        assert_eq!(cloned.end_cap_face_indices, vec![6_u32]);
    }

    // --- task 5b (#2619): AttributeHistory::Sweep + AttributeHistory::Loft ---

    #[test]
    fn attribute_history_sweep_and_loft_variants_construct_and_match() {
        // Sweep wraps SweepOpHistoryRecords (single parent).
        let sweep = AttributeHistory::Sweep(SweepOpHistoryRecords {
            start_cap_face_indices: vec![5],
            end_cap_face_indices: vec![6],
            ..SweepOpHistoryRecords::default()
        });
        match &sweep {
            AttributeHistory::Sweep(records) => {
                assert_eq!(records.start_cap_face_indices, vec![5_u32]);
                assert_eq!(records.end_cap_face_indices, vec![6_u32]);
            }
            _ => panic!("expected AttributeHistory::Sweep"),
        }

        // Loft wraps LoftOpHistoryRecords (multi-parent).
        let loft = AttributeHistory::Loft(LoftOpHistoryRecords {
            start_cap_face_indices: vec![1],
            end_cap_face_indices: vec![2],
            ..LoftOpHistoryRecords::default()
        });
        match &loft {
            AttributeHistory::Loft(records) => {
                assert_eq!(records.start_cap_face_indices, vec![1_u32]);
                assert_eq!(records.end_cap_face_indices, vec![2_u32]);
            }
            _ => panic!("expected AttributeHistory::Loft"),
        }
    }

    #[test]
    fn attribute_history_sweep_and_loft_variants_distinct_from_extrude_revolve_none() {
        let none = AttributeHistory::None;
        let extrude = AttributeHistory::Extrude(SweepOpHistoryRecords::default());
        let revolve = AttributeHistory::Revolve(SweepOpHistoryRecords::default());
        let sweep = AttributeHistory::Sweep(SweepOpHistoryRecords::default());
        let loft = AttributeHistory::Loft(LoftOpHistoryRecords::default());

        assert_ne!(sweep, none);
        assert_ne!(sweep, extrude);
        assert_ne!(sweep, revolve);
        assert_ne!(sweep, loft);

        assert_ne!(loft, none);
        assert_ne!(loft, extrude);
        assert_ne!(loft, revolve);
    }

    #[test]
    fn attribute_history_sweep_and_loft_clone_round_trips() {
        let sweep = AttributeHistory::Sweep(SweepOpHistoryRecords {
            start_cap_face_indices: vec![1, 2],
            end_cap_face_indices: vec![3],
            ..SweepOpHistoryRecords::default()
        });
        let cloned = sweep.clone();
        assert_eq!(sweep, cloned);

        let loft = AttributeHistory::Loft(LoftOpHistoryRecords {
            start_cap_face_indices: vec![4],
            end_cap_face_indices: vec![5, 6],
            ..LoftOpHistoryRecords::default()
        });
        let cloned = loft.clone();
        assert_eq!(loft, cloned);
    }

    #[test]
    fn attribute_history_sweep_and_loft_debug_format() {
        let dbg = format!(
            "{:?}",
            AttributeHistory::Sweep(SweepOpHistoryRecords::default())
        );
        assert!(dbg.contains("Sweep"), "expected Sweep in {dbg}");
        let dbg = format!(
            "{:?}",
            AttributeHistory::Loft(LoftOpHistoryRecords::default())
        );
        assert!(dbg.contains("Loft"), "expected Loft in {dbg}");
    }

    #[test]
    fn geometry_kernel_execute_with_history_default_returns_none_for_sweep_loft_ops() {
        // Verify the default `execute_with_history` impl on `GeometryKernel`
        // still returns AttributeHistory::None for Sweep/Loft ops on
        // non-overriding kernels — task 5b does not change the default.
        struct ExecuteOnlyKernel {
            next_id: u64,
        }

        impl GeometryKernel for ExecuteOnlyKernel {
            fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
                let id = self.next_id;
                self.next_id += 1;
                Ok(GeometryHandle {
                    id: GeometryHandleId(id),
                    repr: Some(BRepKind::Solid),
                })
            }

            fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
                unimplemented!()
            }

            fn export(
                &self,
                _handle: GeometryHandleId,
                _format: ExportFormat,
                _writer: &mut dyn std::io::Write,
            ) -> Result<(), ExportError> {
                unimplemented!()
            }

            fn tessellate(
                &self,
                _handle: GeometryHandleId,
                _tolerance: f64,
            ) -> Result<Mesh, TessError> {
                unimplemented!()
            }
        }

        let mut kernel = ExecuteOnlyKernel { next_id: 1 };

        let op = GeometryOp::Sweep {
            profile: GeometryHandleId(99),
            path: GeometryHandleId(100),
        };
        let (_handle, history) = kernel
            .execute_with_history(&op)
            .expect("default execute_with_history must succeed");
        assert_eq!(history, AttributeHistory::None);

        let op = GeometryOp::Loft {
            profiles: vec![GeometryHandleId(99), GeometryHandleId(100)],
        };
        let (_handle, history) = kernel
            .execute_with_history(&op)
            .expect("default execute_with_history must succeed");
        assert_eq!(history, AttributeHistory::None);
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

    #[test]
    fn topology_attribute_full_construction_pattern_match() {
        let attr = TopologyAttribute {
            feature_id: FeatureId::new("Boss#realization[0]"),
            role: Role::Cap(CapKind::Top),
            local_index: 4,
            user_label: Some("top_face".to_string()),
            mod_history: vec![ModEntry {
                splitting_feature_id: FeatureId::new("Slot#realization[0]"),
                split_index: 1,
            }],
        };
        let TopologyAttribute {
            feature_id,
            role,
            local_index,
            user_label,
            mod_history,
        } = &attr;
        assert_eq!(*feature_id, FeatureId::new("Boss#realization[0]"));
        assert_eq!(*role, Role::Cap(CapKind::Top));
        assert_eq!(*local_index, 4);
        assert_eq!(user_label.as_deref(), Some("top_face"));
        assert_eq!(mod_history.len(), 1);
    }

    #[test]
    fn topology_attribute_default_no_label_no_history() {
        let attr = TopologyAttribute {
            feature_id: FeatureId::new("Boss#realization[0]"),
            role: Role::Side,
            local_index: 0,
            user_label: None,
            mod_history: Vec::new(),
        };
        assert!(attr.user_label.is_none());
        assert!(attr.mod_history.is_empty());
    }

    #[test]
    fn topology_attribute_equality_field_by_field() {
        let baseline = TopologyAttribute {
            feature_id: FeatureId::new("X#realization[0]"),
            role: Role::Side,
            local_index: 1,
            user_label: None,
            mod_history: Vec::new(),
        };
        let same = baseline.clone();
        assert_eq!(baseline, same);

        let mut diff_feature = baseline.clone();
        diff_feature.feature_id = FeatureId::new("Y#realization[0]");
        assert_ne!(baseline, diff_feature);

        let mut diff_role = baseline.clone();
        diff_role.role = Role::NewEdge;
        assert_ne!(baseline, diff_role);

        let mut diff_idx = baseline.clone();
        diff_idx.local_index = 2;
        assert_ne!(baseline, diff_idx);

        let mut diff_label = baseline.clone();
        diff_label.user_label = Some("foo".into());
        assert_ne!(baseline, diff_label);

        let mut diff_history = baseline.clone();
        diff_history.mod_history = vec![ModEntry {
            splitting_feature_id: FeatureId::new("S#realization[0]"),
            split_index: 0,
        }];
        assert_ne!(baseline, diff_history);
    }

    #[test]
    fn topology_attribute_clone_preserves_label_and_history() {
        let attr = TopologyAttribute {
            feature_id: FeatureId::new("Boss#realization[0]"),
            role: Role::Cap(CapKind::Bottom),
            local_index: 2,
            user_label: Some("bottom".to_string()),
            mod_history: vec![
                ModEntry {
                    splitting_feature_id: FeatureId::new("S1#realization[0]"),
                    split_index: 0,
                },
                ModEntry {
                    splitting_feature_id: FeatureId::new("S2#realization[0]"),
                    split_index: 1,
                },
            ],
        };
        let cloned = attr.clone();
        assert_eq!(attr, cloned);
        assert_eq!(cloned.user_label.as_deref(), Some("bottom"));
        assert_eq!(cloned.mod_history.len(), 2);
    }

    #[test]
    fn same_parent_as_returns_true_iff_parent_key_fields_match_excluding_mod_history() {
        // Baseline: two attributes sharing identical (feature_id, role, local_index,
        // user_label) but with DIFFERENT mod_history. The split-children signature.
        let a = TopologyAttribute {
            feature_id: FeatureId::new("Boss#realization[0]"),
            role: Role::Side,
            local_index: 7,
            user_label: Some("seam".to_string()),
            mod_history: Vec::new(),
        };
        let b = TopologyAttribute {
            feature_id: FeatureId::new("Boss#realization[0]"),
            role: Role::Side,
            local_index: 7,
            user_label: Some("seam".to_string()),
            mod_history: vec![ModEntry {
                splitting_feature_id: FeatureId::new("Fuse#realization[0]"),
                split_index: 0,
            }],
        };
        // mod_history differs but parent-key fields all match → predicate true.
        assert!(a.same_parent_as(&b));
        // Symmetry: predicate is order-independent.
        assert!(b.same_parent_as(&a));

        // Diverging on each parent-key field in turn flips the predicate to false.
        let mut diff_feature = b.clone();
        diff_feature.feature_id = FeatureId::new("Slot#realization[0]");
        assert!(!a.same_parent_as(&diff_feature));

        let mut diff_role = b.clone();
        diff_role.role = Role::NewEdge;
        assert!(!a.same_parent_as(&diff_role));

        let mut diff_idx = b.clone();
        diff_idx.local_index = 8;
        assert!(!a.same_parent_as(&diff_idx));

        let mut diff_label = b.clone();
        diff_label.user_label = Some("rim".to_string());
        assert!(!a.same_parent_as(&diff_label));

        // None vs Some on user_label is also a parent-key difference.
        let mut diff_label_none = b.clone();
        diff_label_none.user_label = None;
        assert!(!a.same_parent_as(&diff_label_none));
    }

    fn make_attr(feature: &str, idx: u32) -> TopologyAttribute {
        TopologyAttribute {
            feature_id: FeatureId::new(feature),
            role: Role::Side,
            local_index: idx,
            user_label: None,
            mod_history: Vec::new(),
        }
    }

    #[test]
    fn topology_attribute_table_default_is_empty() {
        let table = TopologyAttributeTable::default();
        assert!(table.is_empty());
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn topology_attribute_table_record_then_lookup() {
        let mut table = TopologyAttributeTable::default();
        let attr = make_attr("F#realization[0]", 0);
        table.record(GeometryHandleId(1), attr.clone());
        assert_eq!(table.lookup(GeometryHandleId(1)), Some(&attr));
        assert_eq!(table.len(), 1);
        assert!(!table.is_empty());
    }

    #[test]
    fn topology_attribute_table_lookup_unknown_returns_none() {
        let mut table = TopologyAttributeTable::default();
        table.record(GeometryHandleId(1), make_attr("F#realization[0]", 0));
        assert_eq!(table.lookup(GeometryHandleId(99)), None);
    }

    #[test]
    fn topology_attribute_table_record_overwrites_last_write_wins() {
        let mut table = TopologyAttributeTable::default();
        let first = make_attr("F#realization[0]", 0);
        let second = make_attr("G#realization[0]", 7);
        table.record(GeometryHandleId(1), first);
        table.record(GeometryHandleId(1), second.clone());
        assert_eq!(table.lookup(GeometryHandleId(1)), Some(&second));
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn topology_attribute_table_iter_yields_all_recorded_entries() {
        let mut table = TopologyAttributeTable::default();
        let attr0 = make_attr("F#realization[0]", 0);
        let attr1 = make_attr("F#realization[0]", 1);
        let attr2 = make_attr("G#realization[0]", 0);
        table.record(GeometryHandleId(1), attr0.clone());
        table.record(GeometryHandleId(2), attr1.clone());
        table.record(GeometryHandleId(3), attr2.clone());

        // iter() must yield exactly len() entries.
        assert_eq!(table.iter().count(), 3);

        // Collect into a HashMap so membership is order-agnostic
        // (TopologyAttributeTable iteration order is unspecified — HashMap-backed).
        let collected: std::collections::HashMap<GeometryHandleId, &TopologyAttribute> =
            table.iter().collect();
        assert_eq!(collected.len(), 3);
        assert_eq!(collected.get(&GeometryHandleId(1)), Some(&&attr0));
        assert_eq!(collected.get(&GeometryHandleId(2)), Some(&&attr1));
        assert_eq!(collected.get(&GeometryHandleId(3)), Some(&&attr2));
    }

    #[test]
    fn boolean_op_parents_binary_constructor_and_accessors() {
        let lf: Vec<GeometryHandleId> = vec![GeometryHandleId(1), GeometryHandleId(2)];
        let rf: Vec<GeometryHandleId> = vec![GeometryHandleId(3), GeometryHandleId(4)];
        let le: Vec<GeometryHandleId> = vec![GeometryHandleId(5)];
        let re: Vec<GeometryHandleId> = vec![GeometryHandleId(6)];

        let parents = BooleanOpParents::Binary {
            faces: [&lf, &rf],
            edges: [&le, &re],
        };

        assert_eq!(parents.face_slices().len(), 2);
        assert_eq!(parents.face_slices()[0], &lf[..]);
        assert_eq!(parents.face_slices()[1], &rf[..]);

        assert_eq!(parents.edge_slices().len(), 2);
        assert_eq!(parents.edge_slices()[0], &le[..]);
        assert_eq!(parents.edge_slices()[1], &re[..]);
    }

    #[test]
    fn boolean_op_parents_nary_constructor_and_accessors() {
        let f0: Vec<GeometryHandleId> = vec![GeometryHandleId(1)];
        let f1: Vec<GeometryHandleId> = vec![GeometryHandleId(2), GeometryHandleId(3)];
        let f2: Vec<GeometryHandleId> = vec![];
        let e0: Vec<GeometryHandleId> = vec![GeometryHandleId(10)];
        let e1: Vec<GeometryHandleId> = vec![];
        let e2: Vec<GeometryHandleId> = vec![GeometryHandleId(11), GeometryHandleId(12)];

        let face_inputs: [&[GeometryHandleId]; 3] = [&f0, &f1, &f2];
        let edge_inputs: [&[GeometryHandleId]; 3] = [&e0, &e1, &e2];

        let parents = BooleanOpParents::NAry {
            faces: &face_inputs,
            edges: &edge_inputs,
        };

        assert_eq!(parents.face_slices().len(), 3);
        assert_eq!(parents.face_slices()[0], &f0[..]);
        assert_eq!(parents.face_slices()[1], &f1[..]);
        assert_eq!(parents.face_slices()[2], &f2[..]);

        assert_eq!(parents.edge_slices().len(), 3);
        assert_eq!(parents.edge_slices()[0], &e0[..]);
        assert_eq!(parents.edge_slices()[1], &e1[..]);
        assert_eq!(parents.edge_slices()[2], &e2[..]);
    }

    #[test]
    fn boolean_op_parents_try_nary_accepts_matched_lengths() {
        let f0: Vec<GeometryHandleId> = vec![GeometryHandleId(1)];
        let f1: Vec<GeometryHandleId> = vec![GeometryHandleId(2), GeometryHandleId(3)];
        let f2: Vec<GeometryHandleId> = vec![];
        let e0: Vec<GeometryHandleId> = vec![GeometryHandleId(10)];
        let e1: Vec<GeometryHandleId> = vec![];
        let e2: Vec<GeometryHandleId> = vec![GeometryHandleId(11), GeometryHandleId(12)];

        let face_inputs: [&[GeometryHandleId]; 3] = [&f0, &f1, &f2];
        let edge_inputs: [&[GeometryHandleId]; 3] = [&e0, &e1, &e2];

        let parents = BooleanOpParents::try_nary(&face_inputs, &edge_inputs)
            .expect("matched-length inputs should succeed");

        assert_eq!(parents.face_slices().len(), 3);
        assert_eq!(parents.face_slices()[0], &f0[..]);
        assert_eq!(parents.face_slices()[1], &f1[..]);
        assert_eq!(parents.face_slices()[2], &f2[..]);

        assert_eq!(parents.edge_slices().len(), 3);
        assert_eq!(parents.edge_slices()[0], &e0[..]);
        assert_eq!(parents.edge_slices()[1], &e1[..]);
        assert_eq!(parents.edge_slices()[2], &e2[..]);
    }

    #[test]
    fn boolean_op_parents_try_nary_rejects_length_mismatch() {
        let f0: Vec<GeometryHandleId> = vec![GeometryHandleId(1)];
        let face_inputs: [&[GeometryHandleId]; 1] = [&f0];
        let e0: Vec<GeometryHandleId> = vec![GeometryHandleId(10)];
        let e1: Vec<GeometryHandleId> = vec![GeometryHandleId(11)];
        let edge_inputs: [&[GeometryHandleId]; 2] = [&e0, &e1];

        let result = BooleanOpParents::try_nary(&face_inputs, &edge_inputs);
        assert_eq!(
            result,
            Err(BooleanOpParentsError::LengthMismatch { faces: 1, edges: 2 })
        );
    }

    #[test]
    fn boolean_op_parents_nary_constructor_accepts_matched_lengths() {
        let f0: Vec<GeometryHandleId> = vec![GeometryHandleId(1)];
        let f1: Vec<GeometryHandleId> = vec![GeometryHandleId(2), GeometryHandleId(3)];
        let f2: Vec<GeometryHandleId> = vec![];
        let e0: Vec<GeometryHandleId> = vec![GeometryHandleId(10)];
        let e1: Vec<GeometryHandleId> = vec![];
        let e2: Vec<GeometryHandleId> = vec![GeometryHandleId(11), GeometryHandleId(12)];

        let face_inputs: [&[GeometryHandleId]; 3] = [&f0, &f1, &f2];
        let edge_inputs: [&[GeometryHandleId]; 3] = [&e0, &e1, &e2];

        let parents = BooleanOpParents::nary(&face_inputs, &edge_inputs);

        assert_eq!(parents.face_slices().len(), 3);
        assert_eq!(parents.face_slices()[0], &f0[..]);
        assert_eq!(parents.face_slices()[1], &f1[..]);
        assert_eq!(parents.face_slices()[2], &f2[..]);

        assert_eq!(parents.edge_slices().len(), 3);
        assert_eq!(parents.edge_slices()[0], &e0[..]);
        assert_eq!(parents.edge_slices()[1], &e1[..]);
        assert_eq!(parents.edge_slices()[2], &e2[..]);
    }

    #[test]
    #[should_panic(expected = "faces.len()")]
    fn boolean_op_parents_nary_constructor_panics_on_length_mismatch() {
        // faces.len() == 1, edges.len() == 2 → should panic
        BooleanOpParents::nary(&[&[][..]], &[&[][..], &[][..]]);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "faces.len()")]
    fn boolean_op_parents_face_slices_debug_asserts_length_mismatch() {
        // Direct-literal NAry construction with mismatched lengths: faces.len() == 1, edges.len() == 2.
        // Calling face_slices() must panic in debug builds via the shared helper.
        let parents = BooleanOpParents::NAry {
            faces: &[&[][..]],
            edges: &[&[][..], &[][..]],
        };
        let _ = parents.face_slices();
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "faces.len()")]
    fn boolean_op_parents_edge_slices_debug_asserts_length_mismatch() {
        // Direct-literal NAry construction with mismatched lengths.
        // Calling edge_slices() must panic in debug builds via the shared helper.
        let parents = BooleanOpParents::NAry {
            faces: &[&[][..]],
            edges: &[&[][..], &[][..]],
        };
        let _ = parents.edge_slices();
    }

    /// Verify that `BRepKind` (renamed from `ReprKind`) retains `Hash + Eq + Copy + Debug`
    /// so it can act as a `HashMap` key and be compared / logged by callers.
    ///
    /// All seven B-rep sub-shape variants must be pairwise distinct.
    #[test]
    fn b_rep_kind_variants_round_trip_through_hashmap_key() {
        use std::collections::HashMap;
        let variants = [
            BRepKind::Solid,
            BRepKind::Shell,
            BRepKind::Wire,
            BRepKind::Compound,
            BRepKind::Edge,
            BRepKind::Face,
            BRepKind::Vertex,
        ];

        // All variants are pairwise distinct.
        for i in 0..variants.len() {
            for j in 0..variants.len() {
                if i != j {
                    assert_ne!(
                        variants[i], variants[j],
                        "expected distinct variants at {i} and {j}"
                    );
                }
            }
        }

        // All seven variants survive a HashMap round-trip (requires Hash + Eq).
        let mut map: HashMap<BRepKind, u32> = HashMap::new();
        for (idx, v) in variants.iter().enumerate() {
            map.insert(*v, idx as u32); // *v requires Copy
        }
        assert_eq!(
            map.len(),
            7,
            "all 7 BRepKind variants must be stored as distinct keys"
        );
        for (idx, v) in variants.iter().enumerate() {
            assert_eq!(
                map[v], idx as u32,
                "HashMap lookup must recover inserted value for {v:?}"
            );
        }
    }

    /// Verify that the v0.3 multi-kernel `ReprKind` (BRep | Mesh | Sdf | Voxel |
    /// VolumeMesh) has `Hash + Eq + Copy + Debug` and that all five variants are
    /// pairwise distinct.
    ///
    /// The compile-time `match` arm at the end proves exhaustiveness — any future
    /// variant addition will cause a compile error here, prompting the developer to
    /// update the test and the RealizationCache's handling.
    #[test]
    fn repr_kind_kernel_family_variants_round_trip_through_hashmap_key() {
        use std::collections::HashMap;
        let variants = [
            ReprKind::BRep,
            ReprKind::Mesh,
            ReprKind::Sdf,
            ReprKind::Voxel,
            ReprKind::VolumeMesh,
        ];

        // All five variants are pairwise distinct (10 pairs).
        for i in 0..variants.len() {
            for j in 0..variants.len() {
                if i != j {
                    assert_ne!(
                        variants[i], variants[j],
                        "expected distinct variants at {i} and {j}"
                    );
                }
            }
        }

        // All five variants survive a HashMap round-trip (requires Hash + Eq + Copy).
        let mut map: HashMap<ReprKind, u32> = HashMap::new();
        for (idx, v) in variants.iter().enumerate() {
            map.insert(*v, idx as u32); // *v requires Copy
        }
        assert_eq!(
            map.len(),
            5,
            "all 5 ReprKind variants must be stored as distinct keys"
        );
        for (idx, v) in variants.iter().enumerate() {
            assert_eq!(
                map[v], idx as u32,
                "HashMap lookup must recover inserted value for {v:?}"
            );
        }

        // Compile-time exhaustiveness check: this match must cover all variants.
        // If a new variant is added, this will fail to compile.
        let v = ReprKind::BRep;
        match v {
            ReprKind::BRep => {}
            ReprKind::Mesh => {}
            ReprKind::Sdf => {}
            ReprKind::Voxel => {}
            ReprKind::VolumeMesh => {}
        }
    }

    /// Verify that the v0.2 multi-kernel `Operation` enum (Booleans, Primitives,
    /// Modify, Transform, Pattern, Sweep, Curve, Convert) has `Hash + Eq + Copy +
    /// Debug` and that all variants are pairwise distinct.
    ///
    /// Mirrors the shape of `repr_kind_kernel_family_variants_round_trip_through_hashmap_key`:
    /// pairwise-distinct + `HashMap<Operation, _>` round-trip + a final compile-time
    /// exhaustive `match` arm. Any future variant addition will cause a compile
    /// error here, prompting the developer to update the test, the dispatcher's
    /// BFS expansion logic, and any kernel adapters' descriptors.
    #[test]
    fn operation_variants_round_trip_through_hashmap_key() {
        use std::collections::HashMap;
        let variants = [
            // Booleans (3)
            Operation::BooleanUnion,
            Operation::BooleanDifference,
            Operation::BooleanIntersection,
            // Primitives (4)
            Operation::PrimitiveBox,
            Operation::PrimitiveCylinder,
            Operation::PrimitiveSphere,
            Operation::PrimitiveTube,
            // Modify (5)
            Operation::ModifyFillet,
            Operation::ModifyChamfer,
            Operation::ModifyShell,
            Operation::ModifyDraft,
            Operation::ModifyThicken,
            // Transform (4)
            Operation::TransformTranslate,
            Operation::TransformRotate,
            Operation::TransformScale,
            Operation::TransformRotateAround,
            // Pattern (5)
            Operation::PatternLinear,
            Operation::PatternCircular,
            Operation::PatternMirror,
            Operation::PatternLinear2D,
            Operation::PatternArbitrary,
            // Sweep (8)
            Operation::SweepLoft,
            Operation::SweepExtrude,
            Operation::SweepRevolve,
            Operation::SweepSweep,
            Operation::SweepExtrudeSymmetric,
            Operation::SweepSweepGuided,
            Operation::SweepLoftGuided,
            Operation::SweepPipe,
            // Curve (6)
            Operation::CurveLineSegment,
            Operation::CurveArc,
            Operation::CurveHelix,
            Operation::CurveInterpCurve,
            Operation::CurveBezierCurve,
            Operation::CurveNurbsCurve,
            // Convert (5 — one per ReprKind)
            Operation::Convert {
                from: ReprKind::BRep,
            },
            Operation::Convert {
                from: ReprKind::Mesh,
            },
            Operation::Convert {
                from: ReprKind::Sdf,
            },
            Operation::Convert {
                from: ReprKind::Voxel,
            },
            Operation::Convert {
                from: ReprKind::VolumeMesh,
            },
        ];

        // (No count-pinning assertion: the compile-time exhaustive `match`
        // below already enforces that every variant is covered, while a
        // hard-coded count would force every kernel-adapter task that
        // adds/renames a variant to also touch this number — pure lock-in
        // friction without checking anything the match doesn't already
        // catch.)

        // All variants are pairwise distinct.
        for i in 0..variants.len() {
            for j in 0..variants.len() {
                if i != j {
                    assert_ne!(
                        variants[i], variants[j],
                        "expected distinct variants at {i} and {j}"
                    );
                }
            }
        }

        // All variants survive a HashMap round-trip (requires Hash + Eq + Copy).
        let mut map: HashMap<Operation, u32> = HashMap::new();
        for (idx, v) in variants.iter().enumerate() {
            map.insert(*v, idx as u32); // *v requires Copy
        }
        assert_eq!(
            map.len(),
            variants.len(),
            "all {} Operation variants must be stored as distinct keys",
            variants.len()
        );
        for (idx, v) in variants.iter().enumerate() {
            assert_eq!(
                map[v], idx as u32,
                "HashMap lookup must recover inserted value for {v:?}"
            );
        }

        // Compile-time exhaustiveness check: this match must cover all variants.
        // If a new variant is added, this will fail to compile.
        let v = Operation::BooleanUnion;
        match v {
            Operation::BooleanUnion => {}
            Operation::BooleanDifference => {}
            Operation::BooleanIntersection => {}
            Operation::PrimitiveBox => {}
            Operation::PrimitiveCylinder => {}
            Operation::PrimitiveSphere => {}
            Operation::PrimitiveTube => {}
            Operation::ModifyFillet => {}
            Operation::ModifyChamfer => {}
            Operation::ModifyShell => {}
            Operation::ModifyDraft => {}
            Operation::ModifyThicken => {}
            Operation::TransformTranslate => {}
            Operation::TransformRotate => {}
            Operation::TransformScale => {}
            Operation::TransformRotateAround => {}
            Operation::PatternLinear => {}
            Operation::PatternCircular => {}
            Operation::PatternMirror => {}
            Operation::PatternLinear2D => {}
            Operation::PatternArbitrary => {}
            Operation::SweepLoft => {}
            Operation::SweepExtrude => {}
            Operation::SweepRevolve => {}
            Operation::SweepSweep => {}
            Operation::SweepExtrudeSymmetric => {}
            Operation::SweepSweepGuided => {}
            Operation::SweepLoftGuided => {}
            Operation::SweepPipe => {}
            Operation::CurveLineSegment => {}
            Operation::CurveArc => {}
            Operation::CurveHelix => {}
            Operation::CurveInterpCurve => {}
            Operation::CurveBezierCurve => {}
            Operation::CurveNurbsCurve => {}
            Operation::Convert { from: _ } => {}
        }
    }

    /// `CapabilityDescriptor::default()` must produce an empty `supports` table.
    /// Locks the `Default` derive contract — kernel adapters rely on starting
    /// from an empty descriptor and pushing `(op, repr)` entries.
    #[test]
    fn capability_descriptor_default_is_empty() {
        let d = CapabilityDescriptor::default();
        assert!(
            d.supports.is_empty(),
            "default descriptor must have empty supports table"
        );
    }

    /// `descriptor.supports(op, repr)` performs an exact-pair match against the
    /// underlying `supports: Vec<(Operation, ReprKind)>`. The test descriptor
    /// claims `(BooleanUnion, Mesh)` and `(Convert{from: BRep}, Mesh)`. Probes:
    /// (a) declared pair → true, (b) op declared but mismatched repr → false,
    /// (c) op never declared → false.
    #[test]
    fn capability_descriptor_supports_lookup() {
        let d = CapabilityDescriptor {
            supports: vec![
                (Operation::BooleanUnion, ReprKind::Mesh),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Mesh,
                ),
            ],
        };

        // Declared pair → true.
        assert!(
            d.supports(Operation::BooleanUnion, ReprKind::Mesh),
            "(BooleanUnion, Mesh) is declared, expected supports() to return true"
        );
        assert!(
            d.supports(
                Operation::Convert {
                    from: ReprKind::BRep
                },
                ReprKind::Mesh
            ),
            "(Convert{{from: BRep}}, Mesh) is declared, expected supports() to return true"
        );

        // Op declared but mismatched output repr → false.
        assert!(
            !d.supports(Operation::BooleanUnion, ReprKind::BRep),
            "(BooleanUnion, BRep) NOT declared, expected supports() to return false"
        );

        // Op never declared → false.
        assert!(
            !d.supports(Operation::BooleanDifference, ReprKind::Mesh),
            "(BooleanDifference, Mesh) NOT declared, expected supports() to return false"
        );

        // Convert with a different `from` is a distinct entry — not a match.
        assert!(
            !d.supports(
                Operation::Convert {
                    from: ReprKind::Mesh
                },
                ReprKind::Mesh
            ),
            "Convert{{from: Mesh}} != Convert{{from: BRep}}, expected supports() to return false"
        );
    }

    /// `CapabilityDescriptor::supports_any_repr` returns `true` iff at least one
    /// entry's *output* repr (the second tuple element) equals `repr`.  The
    /// fixture carries mixed `BRep`, `Mesh`, and `Convert`-output entries.
    /// Probes: (a) repr present as direct output → true, (b) repr present as
    /// Convert output → true, (c) repr absent → false, (d) empty descriptor →
    /// false, (e) Convert-output-vs-from disambiguation: `supports_any_repr(BRep)`
    /// against a descriptor whose only entry is `(Convert{from: BRep}, Mesh)`
    /// must be false — the helper inspects the output (second element), not the
    /// `from` input inside `Operation::Convert`.
    #[test]
    fn capability_descriptor_supports_any_repr_lookup() {
        let d = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (Operation::BooleanUnion, ReprKind::Mesh),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Mesh,
                ),
            ],
        };

        // (a) BRep is present as a direct output repr.
        assert!(
            d.supports_any_repr(ReprKind::BRep),
            "(PrimitiveBox, BRep) output is BRep — supports_any_repr(BRep) must be true"
        );

        // (b) Mesh is present both as a direct output and as a Convert output.
        assert!(
            d.supports_any_repr(ReprKind::Mesh),
            "(BooleanUnion, Mesh) and (Convert{{from:BRep}}, Mesh) both output Mesh — must be true"
        );

        // (c) Sdf is not the output repr of any entry in the fixture.
        assert!(
            !d.supports_any_repr(ReprKind::Sdf),
            "no entry produces Sdf — supports_any_repr(Sdf) must be false"
        );

        // (d) Empty descriptor → false for every repr.
        assert!(
            !CapabilityDescriptor::default().supports_any_repr(ReprKind::BRep),
            "empty descriptor — supports_any_repr must always be false"
        );

        // (e) Convert-output-vs-from disambiguation: the only entry's BRep
        //     appears in the `from` input, NOT the output repr.  The helper
        //     must inspect the output (second tuple element), so the result
        //     is false.
        let convert_only = CapabilityDescriptor {
            supports: vec![(
                Operation::Convert {
                    from: ReprKind::BRep,
                },
                ReprKind::Mesh,
            )],
        };
        assert!(
            !convert_only.supports_any_repr(ReprKind::BRep),
            "BRep is the Convert `from` input, not the output repr — must be false"
        );
    }

    /// `Clone` derive on `CapabilityDescriptor` must round-trip the entire
    /// `supports` table. Locks the `Clone` derive contract.
    #[test]
    fn capability_descriptor_clone_round_trip() {
        let d = CapabilityDescriptor {
            supports: vec![
                (Operation::BooleanUnion, ReprKind::Mesh),
                (Operation::PrimitiveBox, ReprKind::BRep),
            ],
        };
        let cloned = d.clone();
        assert_eq!(
            cloned.supports, d.supports,
            "clone must preserve supports table"
        );
        // PartialEq derive: descriptors compare by structural equality.
        assert_eq!(
            cloned, d,
            "PartialEq derive must hold for cloned descriptor"
        );
    }

    /// Minimal `GeometryKernel` impl that uses every default trait method.
    /// Used by `geometry_kernel_default_attribute_hook_returns_none` to pin the
    /// PRD line 70 contract that "Fidget/OpenVDB don't implement
    /// `KernelAttributeHook` — selectors fall through to computed selectors"
    /// is structurally enforced by the trait DEFAULT, not by per-kernel code.
    ///
    /// We define this locally rather than depending on
    /// `reify_test_support::FailingMockGeometryKernel` because `reify-types`
    /// has no `dev-dependency` on `reify-test-support` (and adding one would
    /// invert the layering — `reify-test-support` depends on `reify-types`).
    struct DefaultsOnlyKernel;

    impl GeometryKernel for DefaultsOnlyKernel {
        fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
            Err(GeometryError::OperationFailed(
                "not used by this test".into(),
            ))
        }
        fn query(&self, _q: &GeometryQuery) -> Result<Value, QueryError> {
            Err(QueryError::QueryFailed("not used by this test".into()))
        }
        fn export(
            &self,
            _h: GeometryHandleId,
            _f: ExportFormat,
            _w: &mut dyn std::io::Write,
        ) -> Result<(), ExportError> {
            Err(ExportError::FormatError("not used by this test".into()))
        }
        fn tessellate(&self, _h: GeometryHandleId, _t: f64) -> Result<Mesh, TessError> {
            Err(TessError::TessellationFailed(
                "not used by this test".into(),
            ))
        }
    }

    /// PRD docs/prds/v0_2/persistent-naming-v2.md line 70 says "Fidget/OpenVDB
    /// don't implement the trait — selectors over SDF or voxel reps fall
    /// through to computed selectors." This contract is structurally enforced
    /// by the trait DEFAULT for `attribute_hook()` returning `None`: any kernel
    /// that does NOT explicitly override the accessor inherits the `None`
    /// fall-through. This test pins the default behaviour against
    /// `DefaultsOnlyKernel` (a kernel that overrides nothing), so a future
    /// regression that flips the default to `Some(...)` would force the
    /// non-overriding kernels (Fidget, OpenVDB, mocks, stubs) to claim a hook
    /// they don't implement — and this test fails immediately.
    #[test]
    fn geometry_kernel_default_attribute_hook_returns_none() {
        let kernel = DefaultsOnlyKernel;
        let kernel_ref: &dyn GeometryKernel = &kernel;
        assert!(
            kernel_ref.attribute_hook().is_none(),
            "GeometryKernel::attribute_hook() default must return None — \
             enforces PRD line 70 'Fidget/OpenVDB selectors fall through to computed selectors' \
             without per-kernel opt-out code",
        );
    }

    /// Mirror of `extract_edges` / `extract_faces` default-impl test (PRD task α):
    /// any kernel that does NOT explicitly override `extract_vertices` must
    /// inherit the trait default and return `Err(QueryError::QueryFailed(_))`.
    /// The exact message text is informational and not part of the public contract —
    /// callers that need to branch on "topology extraction unsupported" should use
    /// a dedicated `QueryError` variant rather than substring matching.
    #[test]
    fn default_geometry_kernel_extract_vertices_returns_topology_not_supported_error() {
        let mut kernel = DefaultsOnlyKernel;
        let result = kernel.extract_vertices(GeometryHandleId(1));
        assert!(
            matches!(result, Err(QueryError::QueryFailed(_))),
            "expected Err(QueryError::QueryFailed(_)), got: {result:?}",
        );
    }

    /// Verify the v0.3 `VolumeMesh` struct and `ElementOrderTag` enum round-trip
    /// through both P1 (4-node tetrahedron) and P2 (10-node tetrahedron) element
    /// orders, and that `Clone` + `Debug` derives are intact so the type can be
    /// stored in `RealizationCache<VolumeMesh>` and logged through tracing.
    ///
    /// The struct is the surface→volume meshing pipeline's output payload —
    /// `reify-solver-elastic` (sibling task #2914) reads it to assemble FEA
    /// stiffness matrices, so the field shape and element-order discriminator
    /// are part of the public API contract.
    #[test]
    fn volume_mesh_struct_round_trip_with_p1_and_p2_element_order_tags() {
        // P1 tetrahedron: 4 corner vertices, 4 indices per element.
        let p1_mesh = VolumeMesh {
            vertices: vec![
                0.0, 0.0, 0.0, // v0
                1.0, 0.0, 0.0, // v1
                0.0, 1.0, 0.0, // v2
                0.0, 0.0, 1.0, // v3
            ],
            tet_indices: vec![0, 1, 2, 3],
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        assert_eq!(
            p1_mesh.vertices.len(),
            12,
            "P1 tet has 4 vertices × 3 floats = 12 flat coordinates"
        );
        assert_eq!(
            p1_mesh.tet_indices.len(),
            4,
            "P1 tet has 4 corner indices (one tetrahedron)"
        );
        assert_eq!(p1_mesh.element_order, ElementOrderTag::P1);

        // P2 tetrahedron: 4 corner + 6 edge-midpoint vertices = 10 nodes per element.
        let p2_mesh = VolumeMesh {
            vertices: vec![
                0.0, 0.0, 0.0, // v0 (corner)
                1.0, 0.0, 0.0, // v1 (corner)
                0.0, 1.0, 0.0, // v2 (corner)
                0.0, 0.0, 1.0, // v3 (corner)
                0.5, 0.0, 0.0, // v4 (mid 0-1)
                0.5, 0.5, 0.0, // v5 (mid 1-2)
                0.0, 0.5, 0.0, // v6 (mid 0-2)
                0.0, 0.0, 0.5, // v7 (mid 0-3)
                0.5, 0.0, 0.5, // v8 (mid 1-3)
                0.0, 0.5, 0.5, // v9 (mid 2-3)
            ],
            tet_indices: vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
            element_order: ElementOrderTag::P2,
            normals: None,
        };
        assert_eq!(
            p2_mesh.tet_indices.len(),
            10,
            "P2 tet has 10 indices per element (4 corner + 6 edge midpoints, Gmsh canonical order)"
        );
        assert_ne!(
            ElementOrderTag::P1,
            ElementOrderTag::P2,
            "P1 and P2 are distinct discriminants"
        );

        // Clone + Debug derives are part of the public surface so the type can
        // be stored in caches and logged through tracing.
        let cloned = p1_mesh.clone();
        let _ = format!("{:?}", cloned);
    }

    // ──────────────────────────────────────────────────────────────────
    // FaceSurfaceKind / EdgeCurveKind — geometry-type filter enums (PRD line 78)
    //
    // Mirror OCCT's `GeomAbs_*` taxonomy: surface kinds and curve kinds
    // are distinct enums (their `Bezier`/`BSpline` arms come from
    // `GeomAbs_BezierSurface`/`GeomAbs_BSplineCurve` and are not
    // interchangeable). The selectors `faces_by_surface_kind` and
    // `edges_by_curve_kind` carry their input-type constraint in the type
    // signature — passing a curve kind to the face selector is a compile
    // error.
    // ──────────────────────────────────────────────────────────────────

    #[test]
    fn face_surface_kind_variants_distinct_and_derive_required_traits() {
        use std::collections::HashSet;

        // Variant-distinctness: each constructor must inhabit a unique
        // discriminant. `HashSet` capacity == 9 (one per variant) proves
        // every arm is reachable and pairwise distinct under `Eq + Hash`.
        let variants = [
            FaceSurfaceKind::Plane,
            FaceSurfaceKind::Cylinder,
            FaceSurfaceKind::Cone,
            FaceSurfaceKind::Sphere,
            FaceSurfaceKind::Torus,
            FaceSurfaceKind::BezierSurface,
            FaceSurfaceKind::BSplineSurface,
            FaceSurfaceKind::OffsetSurface,
            FaceSurfaceKind::Other,
        ];
        let set: HashSet<FaceSurfaceKind> = variants.iter().copied().collect();
        assert_eq!(
            set.len(),
            9,
            "FaceSurfaceKind must have 9 distinct variants — got {:?}",
            set
        );

        // Copy + Debug derives are part of the public surface (Copy lets
        // the selector pass kinds by value through the closure boundary;
        // Debug renders them in error messages).
        let k = FaceSurfaceKind::Plane;
        let copied: FaceSurfaceKind = k; // needs Copy
        let _ = format!("{:?}", copied);
    }

    #[test]
    fn edge_curve_kind_variants_distinct_and_derive_required_traits() {
        use std::collections::HashSet;

        let variants = [
            EdgeCurveKind::Line,
            EdgeCurveKind::Circle,
            EdgeCurveKind::Ellipse,
            EdgeCurveKind::Hyperbola,
            EdgeCurveKind::Parabola,
            EdgeCurveKind::BezierCurve,
            EdgeCurveKind::BSplineCurve,
            EdgeCurveKind::OffsetCurve,
            EdgeCurveKind::Other,
        ];
        let set: HashSet<EdgeCurveKind> = variants.iter().copied().collect();
        assert_eq!(
            set.len(),
            9,
            "EdgeCurveKind must have 9 distinct variants — got {:?}",
            set
        );

        let k = EdgeCurveKind::Line;
        let copied: EdgeCurveKind = k;
        let _ = format!("{:?}", copied);
    }

    #[test]
    fn geometry_query_face_surface_kind_and_edge_curve_kind_variants_exist() {
        // Construct + pattern-match the new GeometryQuery::FaceSurfaceKind variant.
        let face_kind = GeometryQuery::FaceSurfaceKind(GeometryHandleId(17));
        match &face_kind {
            GeometryQuery::FaceSurfaceKind(id) => {
                assert_eq!(*id, GeometryHandleId(17));
            }
            _ => panic!("expected FaceSurfaceKind variant"),
        }

        // Construct + pattern-match the new GeometryQuery::EdgeCurveKind variant.
        let edge_kind = GeometryQuery::EdgeCurveKind(GeometryHandleId(19));
        match &edge_kind {
            GeometryQuery::EdgeCurveKind(id) => {
                assert_eq!(*id, GeometryHandleId(19));
            }
            _ => panic!("expected EdgeCurveKind variant"),
        }
    }

    #[test]
    fn geometry_op_kind_name_returns_stable_token_per_variant() {
        // Every GeometryOp variant must produce a stable token via kind_name().
        // Tokens are the variant names verbatim — any rename breaks this test
        // visibly (compile-time exhaustiveness + runtime string check).
        let cases: &[(&str, GeometryOp)] = &[
            (
                "Box",
                GeometryOp::Box {
                    width: Value::Real(1.0),
                    height: Value::Real(1.0),
                    depth: Value::Real(1.0),
                },
            ),
            (
                "Cylinder",
                GeometryOp::Cylinder {
                    radius: Value::Real(1.0),
                    height: Value::Real(1.0),
                },
            ),
            (
                "Sphere",
                GeometryOp::Sphere {
                    radius: Value::Real(1.0),
                },
            ),
            (
                "Tube",
                GeometryOp::Tube {
                    outer_r: Value::Real(0.01),
                    inner_r: Value::Real(0.005),
                    height: Value::Real(0.02),
                },
            ),
            (
                "Union",
                GeometryOp::Union {
                    left: GeometryHandleId(1),
                    right: GeometryHandleId(2),
                },
            ),
            (
                "Difference",
                GeometryOp::Difference {
                    left: GeometryHandleId(1),
                    right: GeometryHandleId(2),
                },
            ),
            (
                "Intersection",
                GeometryOp::Intersection {
                    left: GeometryHandleId(1),
                    right: GeometryHandleId(2),
                },
            ),
            (
                "Fillet",
                GeometryOp::Fillet {
                    target: GeometryHandleId(1),
                    radius: Value::Real(0.001),
                },
            ),
            (
                "Chamfer",
                GeometryOp::Chamfer {
                    target: GeometryHandleId(1),
                    distance: Value::Real(0.001),
                },
            ),
            (
                "Translate",
                GeometryOp::Translate {
                    target: GeometryHandleId(1),
                    dx: 1.0,
                    dy: 0.0,
                    dz: 0.0,
                },
            ),
            (
                "Rotate",
                GeometryOp::Rotate {
                    target: GeometryHandleId(1),
                    axis: [0.0, 0.0, 1.0],
                    angle_rad: 0.0,
                },
            ),
            (
                "Scale",
                GeometryOp::Scale {
                    target: GeometryHandleId(1),
                    factor: 2.0,
                },
            ),
            (
                "RotateAround",
                GeometryOp::RotateAround {
                    target: GeometryHandleId(1),
                    point: [0.0, 0.0, 0.0],
                    axis: [0.0, 0.0, 1.0],
                    angle_rad: 0.0,
                },
            ),
            (
                "LinearPattern",
                GeometryOp::LinearPattern {
                    target: GeometryHandleId(1),
                    direction: [1.0, 0.0, 0.0],
                    count: 3,
                    spacing: Value::Real(0.01),
                },
            ),
            (
                "CircularPattern",
                GeometryOp::CircularPattern {
                    target: GeometryHandleId(1),
                    axis_origin: [0.0, 0.0, 0.0],
                    axis_dir: [0.0, 0.0, 1.0],
                    count: 4,
                    angle: Value::Real(std::f64::consts::TAU),
                },
            ),
            (
                "Mirror",
                GeometryOp::Mirror {
                    target: GeometryHandleId(1),
                    plane_origin: [0.0, 0.0, 0.0],
                    plane_normal: [1.0, 0.0, 0.0],
                },
            ),
            (
                "LinearPattern2D",
                GeometryOp::LinearPattern2D {
                    target: GeometryHandleId(1),
                    direction1: [1.0, 0.0, 0.0],
                    count1: 2,
                    spacing1: Value::Real(0.01),
                    direction2: [0.0, 1.0, 0.0],
                    count2: 2,
                    spacing2: Value::Real(0.01),
                },
            ),
            (
                "ArbitraryPattern",
                GeometryOp::ArbitraryPattern {
                    target: GeometryHandleId(1),
                    transforms: vec![[0.0, 0.0, 0.0]],
                },
            ),
            (
                "Loft",
                GeometryOp::Loft {
                    profiles: vec![GeometryHandleId(1), GeometryHandleId(2)],
                },
            ),
            (
                "Extrude",
                GeometryOp::Extrude {
                    profile: GeometryHandleId(1),
                    distance: Value::Real(0.01),
                },
            ),
            (
                "Revolve",
                GeometryOp::Revolve {
                    profile: GeometryHandleId(1),
                    axis_origin: [0.0, 0.0, 0.0],
                    axis_dir: [0.0, 0.0, 1.0],
                    angle_rad: std::f64::consts::TAU,
                },
            ),
            (
                "Sweep",
                GeometryOp::Sweep {
                    profile: GeometryHandleId(1),
                    path: GeometryHandleId(2),
                },
            ),
            (
                "Pipe",
                GeometryOp::Pipe {
                    path: GeometryHandleId(1),
                    radius: Value::Real(0.002),
                },
            ),
            (
                "ExtrudeSymmetric",
                GeometryOp::ExtrudeSymmetric {
                    profile: GeometryHandleId(1),
                    distance: Value::Real(0.01),
                },
            ),
            (
                "SweepGuided",
                GeometryOp::SweepGuided {
                    profile: GeometryHandleId(1),
                    path: GeometryHandleId(2),
                    guide: GeometryHandleId(3),
                },
            ),
            (
                "LoftGuided",
                GeometryOp::LoftGuided {
                    profiles: vec![GeometryHandleId(1), GeometryHandleId(2)],
                    guides: vec![GeometryHandleId(3)],
                },
            ),
            (
                "LineSegment",
                GeometryOp::LineSegment {
                    x1: 0.0,
                    y1: 0.0,
                    z1: 0.0,
                    x2: 1.0,
                    y2: 0.0,
                    z2: 0.0,
                },
            ),
            (
                "Arc",
                GeometryOp::Arc {
                    center: [0.0, 0.0, 0.0],
                    radius: 1.0,
                    start_angle: 0.0,
                    end_angle: std::f64::consts::FRAC_PI_2,
                    axis: [0.0, 0.0, 1.0],
                },
            ),
            (
                "Helix",
                GeometryOp::Helix {
                    radius: 0.01,
                    pitch: 0.002,
                    height: 0.02,
                },
            ),
            (
                "InterpCurve",
                GeometryOp::InterpCurve {
                    points: vec![[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0]],
                },
            ),
            (
                "BezierCurve",
                GeometryOp::BezierCurve {
                    control_points: vec![[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [2.0, 0.0, 0.0]],
                },
            ),
            (
                "NurbsCurve",
                GeometryOp::NurbsCurve {
                    control_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
                    weights: vec![1.0, 1.0],
                    knots: vec![0.0, 0.0, 1.0, 1.0],
                    degree: 1,
                },
            ),
            (
                "Draft",
                GeometryOp::Draft {
                    target: GeometryHandleId(1),
                    angle: Value::Real(0.1),
                    plane: GeometryHandleId(2),
                },
            ),
            (
                "Thicken",
                GeometryOp::Thicken {
                    target: GeometryHandleId(1),
                    offset: Value::Real(0.001),
                },
            ),
            (
                "Shell",
                GeometryOp::Shell {
                    target: GeometryHandleId(1),
                    thickness: Value::Real(0.001),
                    faces_to_remove: vec![0],
                },
            ),
        ];
        // Changing this constant forces the test to be updated whenever a
        // variant is added or removed from GeometryOp — compile-time
        // exhaustiveness on kind_name() guarantees correctness, this assertion
        // guarantees the token list here stays in sync.
        const GEOMETRY_OP_VARIANT_COUNT: usize = 35;
        assert_eq!(
            cases.len(),
            GEOMETRY_OP_VARIANT_COUNT,
            "Update `cases` and kind_name() when adding/removing GeometryOp variants",
        );
        for (expected, op) in cases {
            assert_eq!(
                op.kind_name(),
                *expected,
                "kind_name() mismatch for GeometryOp::{expected}"
            );
        }
    }

    #[test]
    fn geometry_query_kind_name_returns_stable_token_per_variant() {
        // Every GeometryQuery variant must produce a stable token via kind_name().
        // Tokens are the variant names verbatim — any rename breaks this test
        // visibly (compile-time exhaustiveness + runtime string check).
        let cases: &[(&str, GeometryQuery)] = &[
            ("Volume", GeometryQuery::Volume(GeometryHandleId(1))),
            (
                "SurfaceArea",
                GeometryQuery::SurfaceArea(GeometryHandleId(1)),
            ),
            ("Centroid", GeometryQuery::Centroid(GeometryHandleId(1))),
            (
                "BoundingBox",
                GeometryQuery::BoundingBox(GeometryHandleId(1)),
            ),
            (
                "Distance",
                GeometryQuery::Distance {
                    from: GeometryHandleId(1),
                    to: GeometryHandleId(2),
                },
            ),
            (
                "MomentOfInertia",
                GeometryQuery::MomentOfInertia {
                    handle: GeometryHandleId(1),
                    axis: [0.0, 0.0, 1.0],
                },
            ),
            (
                "AdjacentFaces",
                GeometryQuery::AdjacentFaces {
                    shape: GeometryHandleId(1),
                    face_index: 0,
                },
            ),
            (
                "AncestorFacesOfEdge",
                GeometryQuery::AncestorFacesOfEdge {
                    shape: GeometryHandleId(1),
                    edge_index: 0,
                },
            ),
            (
                "SharedEdges",
                GeometryQuery::SharedEdges {
                    shape: GeometryHandleId(1),
                    face_a: 0,
                    face_b: 1,
                },
            ),
            (
                "IsWatertight",
                GeometryQuery::IsWatertight(GeometryHandleId(1)),
            ),
            ("IsManifold", GeometryQuery::IsManifold(GeometryHandleId(1))),
            (
                "IsOrientable",
                GeometryQuery::IsOrientable(GeometryHandleId(1)),
            ),
            (
                "CenterOfMass",
                GeometryQuery::CenterOfMass {
                    handle: GeometryHandleId(1),
                    density: 1000.0,
                },
            ),
            (
                "InertiaTensor",
                GeometryQuery::InertiaTensor {
                    handle: GeometryHandleId(1),
                    density: 1000.0,
                },
            ),
            ("EdgeLength", GeometryQuery::EdgeLength(GeometryHandleId(1))),
            (
                "EdgeTangent",
                GeometryQuery::EdgeTangent(GeometryHandleId(1)),
            ),
            ("FaceNormal", GeometryQuery::FaceNormal(GeometryHandleId(1))),
            (
                "FaceSurfaceKind",
                GeometryQuery::FaceSurfaceKind(GeometryHandleId(1)),
            ),
            (
                "EdgeCurveKind",
                GeometryQuery::EdgeCurveKind(GeometryHandleId(1)),
            ),
            ("OwnerBody", GeometryQuery::OwnerBody(GeometryHandleId(1))),
            (
                "ClosestPointOnShape",
                GeometryQuery::ClosestPointOnShape {
                    handle: GeometryHandleId(1),
                    px: 0.0,
                    py: 0.0,
                    pz: 0.0,
                },
            ),
            (
                "PointOnShape",
                GeometryQuery::PointOnShape {
                    handle: GeometryHandleId(1),
                    px: 0.0,
                    py: 0.0,
                    pz: 0.0,
                    tolerance: super::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
                },
            ),
            (
                "SurfaceAngle",
                GeometryQuery::SurfaceAngle {
                    face_a: GeometryHandleId(1),
                    face_b: GeometryHandleId(2),
                },
            ),
        ];
        // Changing this constant forces the test to be updated whenever a
        // variant is added or removed from GeometryQuery — compile-time
        // exhaustiveness on kind_name() guarantees correctness, this assertion
        // guarantees the token list here stays in sync.
        const GEOMETRY_QUERY_VARIANT_COUNT: usize = 23;
        assert_eq!(
            cases.len(),
            GEOMETRY_QUERY_VARIANT_COUNT,
            "Update `cases` and kind_name() when adding/removing GeometryQuery variants",
        );
        for (expected, q) in cases {
            assert_eq!(
                q.kind_name(),
                *expected,
                "kind_name() mismatch for GeometryQuery::{expected}"
            );
        }
    }

    // ──────────────────────────────────────────────────────────────────
    // VolumeMesh::vertex — safe flat-XYZ indexing helper
    //
    // The helper centralises the overflow-safe bounds-check/indexing
    // pattern previously inlined at reify-mesh-morph/src/boundary.rs
    // and (parallel duplicate) laplacian.rs.
    // ──────────────────────────────────────────────────────────────────

    /// Verify `VolumeMesh::vertex` returns `Some([x, y, z])` for valid indices
    /// and `None` for out-of-range or overflow inputs.
    ///
    /// Fixture: 3-node mesh with distinct coordinates so each triple is
    /// unambiguous.  The five sub-cases cover (a) first node, (b) last valid
    /// node, (c) one-past-end, (d) u32::MAX overflow guard, and (e) empty mesh.
    #[test]
    fn volume_mesh_vertex_returns_some_for_valid_indices_and_none_for_out_of_range_or_overflow() {
        let mesh = VolumeMesh {
            vertices: vec![
                1.0, 2.0, 3.0, // v0
                4.0, 5.0, 6.0, // v1
                7.0, 8.0, 9.0, // v2
            ],
            tet_indices: vec![],
            element_order: ElementOrderTag::P1,
            normals: None,
        };

        // (a) first node
        assert_eq!(mesh.vertex(0), Some([1.0, 2.0, 3.0]));
        // (b) last valid node — base = 6, end = 9 == vertices.len()
        assert_eq!(mesh.vertex(2), Some([7.0, 8.0, 9.0]));
        // (c) one past end — base = 9, end = 12 > 9
        assert_eq!(mesh.vertex(3), None);
        // (d) large index — on 32-bit targets checked_mul(3) overflows; on
        //     64-bit (typical CI) it falls through to the `end > len` check.
        //     Either path returns None, which is what matters.
        assert_eq!(mesh.vertex(u32::MAX), None);

        // (e) empty mesh — any index is out of range
        let empty = VolumeMesh {
            vertices: vec![],
            tet_indices: vec![],
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        assert_eq!(empty.vertex(0), None);
    }

    /// Verify `VolumeMesh::vertex_f64` widens f32 → f64 for a valid index
    /// and passes `None` through from `vertex` for an out-of-range index.
    ///
    /// Bounds-check coverage (one-past-end, u32::MAX overflow, empty mesh) is
    /// already exercised by the f32 `vertex` test above; `vertex_f64` delegates
    /// entirely to `vertex` and only maps the widening, so duplicating those
    /// cases here would add maintenance overhead without testing new logic.
    #[test]
    fn volume_mesh_vertex_f64_widens_f32_to_f64_and_passes_through_none() {
        let mesh = VolumeMesh {
            vertices: vec![1.0f32, 2.0, 3.0],
            tet_indices: vec![],
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        // valid index — f32 values widened to f64
        assert_eq!(mesh.vertex_f64(0), Some([1.0_f64, 2.0, 3.0]));
        // out-of-range — None passes through from vertex
        assert_eq!(mesh.vertex_f64(1), None);
    }

    /// Pins the trait-object branch of `ingest_mesh`'s default impl: when
    /// called through a `Box<dyn GeometryKernel>`, `type_name::<Self>()`
    /// resolves to `"dyn GeometryKernel"` rather than the concrete kernel
    /// name.  The observable contract the executor cares about is that the
    /// error payload still contains "does not accept Mesh inputs".
    #[test]
    fn ingest_mesh_default_returns_does_not_accept_via_trait_object() {
        struct StubKernel;
        impl GeometryKernel for StubKernel {
            fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
                Err(GeometryError::OperationFailed("stub".into()))
            }
            fn query(&self, _q: &GeometryQuery) -> Result<Value, QueryError> {
                Err(QueryError::QueryFailed("stub".into()))
            }
            fn export(
                &self,
                _h: GeometryHandleId,
                _f: ExportFormat,
                _w: &mut dyn std::io::Write,
            ) -> Result<(), ExportError> {
                Err(ExportError::FormatError("stub".into()))
            }
            fn tessellate(
                &self,
                _h: GeometryHandleId,
                _t: f64,
            ) -> Result<Mesh, TessError> {
                Err(TessError::TessellationFailed("stub".into()))
            }
        }

        let mut boxed: Box<dyn GeometryKernel> = Box::new(StubKernel);
        let mesh = Mesh { vertices: vec![], indices: vec![], normals: None };
        match boxed.ingest_mesh(&mesh) {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("does not accept Mesh inputs"),
                    "error payload must contain 'does not accept Mesh inputs'; got: {msg:?}",
                );
            }
            other => panic!(
                "expected Err(OperationFailed(_)) from trait-object ingest_mesh; got {other:?}"
            ),
        }
    }
}
