//! CXX bridge to the OCCT C++ wrapper.

#[allow(clippy::module_inception, clippy::too_many_arguments)]
#[cxx::bridge(namespace = "occt")]
pub mod ffi {
    /// 3D point returned from queries.
    struct Point3 {
        x: f64,
        y: f64,
        z: f64,
    }

    /// Bounding box returned from queries.
    struct BBox {
        xmin: f64,
        ymin: f64,
        zmin: f64,
        xmax: f64,
        ymax: f64,
        zmax: f64,
    }

    /// Tessellation result returned across FFI.
    struct TessResult {
        vertices: Vec<f32>,
        indices: Vec<u32>,
        normals: Vec<f32>,
    }

    /// Full 3×3 inertia tensor returned from `query_inertia_tensor`.
    ///
    /// Fields are named m{row}{col} in row-major order (m11 = row 1, col 1).
    /// The tensor is always symmetric (Iij = Iji); off-diagonal entries are
    /// products of inertia and vanish only when the coordinate axes coincide
    /// with the body's principal axes (true for axis-aligned primitives at
    /// their centroid).
    struct InertiaTensor3x3 {
        m11: f64,
        m12: f64,
        m13: f64,
        m21: f64,
        m22: f64,
        m23: f64,
        m31: f64,
        m32: f64,
        m33: f64,
    }

    /// Topology-map cache build counts for an OcctShape.
    ///
    /// Each counter is 0 on a fresh shape and increments to 1 on the first
    /// call that needs that map. Used by integration tests to assert that
    /// repeated queries hit the cache instead of rebuilding the map.
    #[derive(Debug, PartialEq, Eq)]
    struct TopologyCacheBuildCounts {
        face_map_builds: u32,
        edge_map_builds: u32,
        edge_face_map_builds: u32,
    }

    unsafe extern "C++" {
        include!("occt_wrapper.h");

        /// Opaque wrapper around TopoDS_Shape.
        type OcctShape;

        /// Opaque vector of shapes for passing N shapes across FFI.
        type OcctShapeVec;

        // --- OcctShapeVec builder + reader ---
        fn new_shape_vec() -> UniquePtr<OcctShapeVec>;
        fn shape_vec_push(vec: Pin<&mut OcctShapeVec>, shape: &OcctShape);
        fn shape_vec_len(vec: &OcctShapeVec) -> usize;
        fn shape_vec_at(vec: &OcctShapeVec, idx: usize) -> Result<UniquePtr<OcctShape>>;

        // --- Primitive construction ---
        fn make_box(width: f64, height: f64, depth: f64) -> Result<UniquePtr<OcctShape>>;
        fn make_cylinder(radius: f64, height: f64) -> Result<UniquePtr<OcctShape>>;
        fn make_sphere(radius: f64) -> Result<UniquePtr<OcctShape>>;

        // --- Boolean operations ---
        fn boolean_fuse(left: &OcctShape, right: &OcctShape) -> Result<UniquePtr<OcctShape>>;
        fn boolean_cut(left: &OcctShape, right: &OcctShape) -> Result<UniquePtr<OcctShape>>;
        fn boolean_common(left: &OcctShape, right: &OcctShape) -> Result<UniquePtr<OcctShape>>;

        /// Probe whether `a` and `b` have a non-empty BRepAlgoAPI_Common result.
        /// Returns true iff any sub-shape (TopoDS_Iterator::More()) of the common
        /// shape exists. Face-touching pairs count as intersecting.
        fn shapes_intersect(a: &OcctShape, b: &OcctShape) -> Result<bool>;

        // --- Modifications ---
        fn fillet_all_edges(shape: &OcctShape, radius: f64) -> Result<UniquePtr<OcctShape>>;
        fn chamfer_all_edges(shape: &OcctShape, distance: f64) -> Result<UniquePtr<OcctShape>>;

        // --- Transforms ---
        fn translate_shape(
            shape: &OcctShape,
            dx: f64,
            dy: f64,
            dz: f64,
        ) -> Result<UniquePtr<OcctShape>>;
        fn rotate_shape(
            shape: &OcctShape,
            ax: f64,
            ay: f64,
            az: f64,
            angle_rad: f64,
        ) -> Result<UniquePtr<OcctShape>>;
        fn scale_shape(
            shape: &OcctShape,
            factor: f64,
            cx: f64,
            cy: f64,
            cz: f64,
        ) -> Result<UniquePtr<OcctShape>>;
        fn rotate_around_shape(
            shape: &OcctShape,
            px: f64,
            py: f64,
            pz: f64,
            ax: f64,
            ay: f64,
            az: f64,
            angle_rad: f64,
        ) -> Result<UniquePtr<OcctShape>>;

        // --- Mirror / Pattern / Circular pattern ---
        fn mirror_shape(
            shape: &OcctShape,
            ox: f64,
            oy: f64,
            oz: f64,
            nx: f64,
            ny: f64,
            nz: f64,
        ) -> Result<UniquePtr<OcctShape>>;

        fn linear_pattern(
            shape: &OcctShape,
            dx: f64,
            dy: f64,
            dz: f64,
            count: u32,
            spacing: f64,
        ) -> Result<UniquePtr<OcctShape>>;

        fn circular_pattern(
            shape: &OcctShape,
            ox: f64,
            oy: f64,
            oz: f64,
            ax: f64,
            ay: f64,
            az: f64,
            count: u32,
            total_angle: f64,
        ) -> Result<UniquePtr<OcctShape>>;

        fn linear_pattern_2d(
            shape: &OcctShape,
            dx1: f64,
            dy1: f64,
            dz1: f64,
            count1: u32,
            spacing1: f64,
            dx2: f64,
            dy2: f64,
            dz2: f64,
            count2: u32,
            spacing2: f64,
        ) -> Result<UniquePtr<OcctShape>>;

        fn arbitrary_pattern(
            shape: &OcctShape,
            flat_transforms: &Vec<f64>,
            num_transforms: u32,
        ) -> Result<UniquePtr<OcctShape>>;

        // --- Draft ---
        fn draft_shape(
            shape: &OcctShape,
            angle_rad: f64,
            plane_shape: &OcctShape,
        ) -> Result<UniquePtr<OcctShape>>;

        // --- Thicken / Shell ---
        fn thicken_shape(shape: &OcctShape, offset: f64) -> Result<UniquePtr<OcctShape>>;
        fn shell_shape(
            shape: &OcctShape,
            thickness: f64,
            face_indices: &Vec<u32>,
        ) -> Result<UniquePtr<OcctShape>>;

        // --- Wire helpers / Loft ---
        fn make_circle_wire(radius: f64, z_height: f64) -> Result<UniquePtr<OcctShape>>;
        fn make_circle_face(radius: f64, z_height: f64) -> Result<UniquePtr<OcctShape>>;
        fn make_line_wire(
            x1: f64,
            y1: f64,
            z1: f64,
            x2: f64,
            y2: f64,
            z2: f64,
        ) -> Result<UniquePtr<OcctShape>>;

        // --- Curve constructors ---
        fn make_arc_wire(
            cx: f64, cy: f64, cz: f64,
            radius: f64,
            start_angle: f64, end_angle: f64,
            ax: f64, ay: f64, az: f64,
        ) -> Result<UniquePtr<OcctShape>>;
        fn make_helix_wire(
            radius: f64, pitch: f64, height: f64,
        ) -> Result<UniquePtr<OcctShape>>;
        /// Build a polyline wire from N >= 2 points (flat 3*N coord slice).
        /// Produces N-1 line edges.  Stable kernel FFI primitive: polygon-face
        /// backing wires, multi-segment sweep/pipe paths, and
        /// BRepAdaptor_CompCurve composite-wire testing.
        fn make_polyline_wire(
            coords: &[f64], n_points: usize,
        ) -> Result<UniquePtr<OcctShape>>;
        fn make_interp_curve(
            coords: &[f64], n_points: usize,
        ) -> Result<UniquePtr<OcctShape>>;
        fn make_bezier_curve(
            coords: &[f64], n_points: usize,
        ) -> Result<UniquePtr<OcctShape>>;
        fn make_nurbs_curve(
            pole_coords: &[f64], n_poles: usize,
            weights: &[f64],
            flat_knots: &[f64],
            degree: i32,
        ) -> Result<UniquePtr<OcctShape>>;

        fn loft_profiles(profiles: &OcctShapeVec) -> Result<UniquePtr<OcctShape>>;

        // --- Sweep ---
        fn make_pipe(profile: &OcctShape, spine: &OcctShape) -> Result<UniquePtr<OcctShape>>;

        /// Sweep a profile along a spine path with a guide wire
        /// biasing orientation (BRepOffsetAPI_MakePipeShell +
        /// SetMode(guide, /*KeepContact=*/false)).
        fn make_pipe_shell(
            profile: &OcctShape,
            spine: &OcctShape,
            guide: &OcctShape,
        ) -> Result<UniquePtr<OcctShape>>;

        /// Loft through >= 2 section profiles using the first guide as
        /// the spine (BRepOffsetAPI_MakePipeShell). An optional second
        /// guide is applied as an auxiliary-orientation constraint via
        /// SetMode(aux, /*KeepContact=*/false).
        fn loft_guided_profiles(
            profiles: &OcctShapeVec,
            guides: &OcctShapeVec,
        ) -> Result<UniquePtr<OcctShape>>;

        // --- Sweep / Extrude / Revolve ---
        fn make_prism(
            profile: &OcctShape,
            dx: f64,
            dy: f64,
            dz: f64,
        ) -> Result<UniquePtr<OcctShape>>;
        fn make_revolve(
            profile: &OcctShape,
            ox: f64,
            oy: f64,
            oz: f64,
            ax: f64,
            ay: f64,
            az: f64,
            angle_rad: f64,
        ) -> Result<UniquePtr<OcctShape>>;
        fn make_rect_face(
            width: f64,
            height: f64,
            cx: f64,
            cy: f64,
            cz: f64,
        ) -> Result<UniquePtr<OcctShape>>;

        // --- Wire queries ---
        fn wire_start_tangent(wire: &OcctShape) -> Result<Point3>;

        // --- Queries ---
        fn query_volume(shape: &OcctShape) -> Result<f64>;
        fn query_area(shape: &OcctShape) -> Result<f64>;
        fn query_edge_length(shape: &OcctShape) -> Result<f64>;
        /// Unit tangent of `shape` (must be a TopoDS_Edge) sampled at the
        /// midpoint of the edge's parameter range. Direction is sign-arbitrary
        /// (the edge's topological orientation is not honoured): callers
        /// that care about specific orientation should compare both `±t`.
        fn query_edge_tangent(shape: &OcctShape) -> Result<Point3>;
        /// Unit outward normal of `shape` (must be a TopoDS_Face) sampled at
        /// the face's centroid. Honours topological orientation: a REVERSED
        /// face yields the topologically-outward normal.
        fn query_face_normal(shape: &OcctShape) -> Result<Point3>;
        fn query_centroid(shape: &OcctShape) -> Result<Point3>;
        /// Surface-properties centroid for a 2D sub-shape (TopoDS_Face).
        /// Used by the `Centroid` query path when the stored repr is
        /// `ReprKind::Face`, since `query_centroid` (volume-based) returns
        /// the origin for isolated faces with zero enclosed volume.
        fn query_face_centroid(shape: &OcctShape) -> Result<Point3>;
        fn query_bbox(shape: &OcctShape) -> Result<BBox>;

        fn query_distance(shape1: &OcctShape, shape2: &OcctShape) -> Result<f64>;

        /// Minimum BREP distance between `a` and `b` via BRepExtrema_DistShapeShape.
        /// Separate symbol from query_distance for the kinematic-constraints call
        /// site (task 2531; see PRD task 7).
        fn min_clearance(a: &OcctShape, b: &OcctShape) -> Result<f64>;

        fn query_moment_of_inertia(shape: &OcctShape, ax: f64, ay: f64, az: f64) -> Result<f64>;

        /// Compute the full 3×3 inertia tensor (kg·m²) about the centroid.
        ///
        /// Uses `BRepGProp::VolumeProperties` + `GProp_GProps::MatrixOfInertia()`.
        /// Each entry of OCCT's volume-weighted matrix is multiplied by `density`
        /// so the result is the mass-weighted tensor. For a uniform-density solid
        /// with mass m = density·volume, the diagonal entries are the principal
        /// moments; off-diagonals are products of inertia (zero for axis-aligned
        /// shapes). Off-diagonal pairs are averaged so `m_ij == m_ji` is guaranteed
        /// bit-exactly in the returned struct.  A relative-tolerance check (1e-9
        /// relative + 1e-12 absolute floor) is applied before averaging to guard
        /// against gross asymmetry from a future OCCT regression or corrupted shape.
        fn query_inertia_tensor(shape: &OcctShape, density: f64) -> Result<InertiaTensor3x3>;

        /// Return cache build counts for all three topology-map slots of `shape`.
        /// Each counter is 0 on a fresh shape, 1 after first use, and never changes
        /// again (immutable post-construction guarantee on OcctShape).
        fn topology_cache_build_counts(shape: &OcctShape) -> TopologyCacheBuildCounts;

        /// Faces sharing at least one edge with `face_index` (0-based, TopExp order).
        /// Excludes the queried face; deduplicated; returned ascending.
        fn adjacent_faces(shape: &OcctShape, face_index: u32) -> Result<Vec<u32>>;

        /// Edges shared between `face_a_index` and `face_b_index` (0-based,
        /// TopExp order). Empty if the two indices are equal. Deduplicated;
        /// returned ascending. Errors if either index is out of range.
        fn shared_edges(
            shape: &OcctShape,
            face_a_index: u32,
            face_b_index: u32,
        ) -> Result<Vec<u32>>;

        /// Materialize the unique edges of `shape` into an OcctShapeVec
        /// (canonical TopExp::MapShapes order, deduplicated by IsSame).
        fn get_edges(shape: &OcctShape) -> Result<UniquePtr<OcctShapeVec>>;

        /// Materialize the unique faces of `shape` into an OcctShapeVec
        /// (canonical TopExp::MapShapes order, deduplicated by IsSame).
        fn get_faces(shape: &OcctShape) -> Result<UniquePtr<OcctShapeVec>>;

        // --- Conformance queries ---

        /// Check whether `shape` is watertight (closed, no free edges).
        /// Returns false immediately for types other than SOLID/COMPSOLID/SHELL.
        /// COMPOUND is excluded: IsValid() checks topological consistency, not
        /// closure — a compound of open faces can spuriously return true.
        /// Backed by BRepCheck_Analyzer.IsValid() for SOLID/COMPSOLID/SHELL.
        fn is_watertight(shape: &OcctShape) -> Result<bool>;

        /// Check whether every edge of `shape` has at most 2 parent faces.
        /// Backed by walking the cached edge_face_map.
        fn is_manifold(shape: &OcctShape) -> Result<bool>;

        /// Check whether all shells of `shape` are consistently oriented.
        /// Backed by ShapeAnalysis_Shell::CheckOrientedShells(shape, alsofree=false).
        /// Trivially true for shapes with no shells (wires, isolated faces, vertices).
        fn is_orientable(shape: &OcctShape) -> Result<bool>;

        // --- Test fixture helpers ---
        // Exposed (not gated on cfg(test)) so integration-test crates can call them
        // via OcctKernel::store_*_for_test helpers in lib.rs.

        /// Three faces sharing one edge → non-manifold compound.
        fn make_nonmanifold_compound_for_test() -> Result<UniquePtr<OcctShape>>;

        /// 10×10×10 mm box missing one face → open shell inside a solid.
        fn make_malformed_solid_for_test() -> Result<UniquePtr<OcctShape>>;

        /// Two faces sharing one edge with identical orientation → non-orientable shell.
        fn make_nonorientable_shell_for_test() -> Result<UniquePtr<OcctShape>>;

        /// Closed shell extracted from a 10×10×10 mm box → TopAbs_SHELL, all predicates true.
        fn make_closed_shell_for_test() -> Result<UniquePtr<OcctShape>>;

        /// Straight edge (0,0,0)→(10mm,0,0) → TopAbs_EDGE; type-guard fires for watertight.
        fn make_edge_for_test() -> Result<UniquePtr<OcctShape>>;

        /// Single vertex at origin → TopAbs_VERTEX; type-guard fires for watertight.
        fn make_vertex_for_test() -> Result<UniquePtr<OcctShape>>;

        /// CompSolid wrapping one 10×10×10 mm box → TopAbs_COMPSOLID; type-guard passes.
        fn make_compsolid_for_test() -> Result<UniquePtr<OcctShape>>;

        // --- Export ---
        fn export_step(shape: &OcctShape) -> Result<String>;

        // --- BRep serialization ---
        fn serialize_brep(shape: &OcctShape) -> Result<String>;
        fn deserialize_brep(data: &CxxString) -> Result<UniquePtr<OcctShape>>;

        // --- Tessellation ---
        fn tessellate_shape(shape: &OcctShape, tolerance: f64) -> Result<TessResult>;

    }
}
