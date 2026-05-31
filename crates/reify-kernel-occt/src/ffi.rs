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

    /// Result of `revolve_synthesis_post_sort_for_test`: the deduplicated
    /// flat record buffer and the count of dropped duplicate records.
    ///
    /// Mirrors the `TopologyCacheBuildCounts` compound-return pattern.
    /// Defined here (in the cxx bridge) so cxx generates matching C++ and
    /// Rust types automatically.
    struct RevolveSynthesisPostSortResult {
        /// Deduplicated flat `(parent_index, parent_subshape_index,
        /// result_subshape_index)` triples, stable-sorted by
        /// `parent_subshape_index`, with duplicates removed.
        output: Vec<u32>,
        /// Number of records dropped because their `parent_subshape_index`
        /// equalled the preceding record's (after stable-sort).
        duplicate_count: u32,
    }

    /// Rigid-body transform: unit quaternion + translation. POD twin of
    /// `crate::Transform3` (always-compiled public type) for cxx-bridge transit.
    ///
    /// Field order matches `Transform3` exactly: `{ qw, qx, qy, qz, tx, ty, tz }`.
    /// On the C++ side the conversion to `gp_Quaternion` is explicit:
    /// `gp_Quaternion(t.qx, t.qy, t.qz, t.qw)` — OCCT takes `(x, y, z, w)`.
    struct Transform3Props {
        qw: f64,
        qx: f64,
        qy: f64,
        qz: f64,
        tx: f64,
        ty: f64,
        tz: f64,
    }

    /// Curvature properties at a parametric point on a face surface.
    ///
    /// Returned by `curvature_at`. All direction vectors are unit tangent
    /// vectors lying in the tangent plane of the surface at `(u, v)`.
    struct CurvatureProps {
        /// Gaussian curvature K = κ₁·κ₂. Invariant under normal reversal.
        gaussian: f64,
        /// Mean curvature H = (κ₁ + κ₂) / 2. Sign follows outward normal
        /// convention (negated for TopAbs_REVERSED faces).
        mean: f64,
        /// Minimum principal curvature κ_min ≤ κ_max.
        kappa_min: f64,
        /// Maximum principal curvature κ_max ≥ κ_min.
        kappa_max: f64,
        /// Principal direction for κ_min (unit tangent vector).
        dir_min: Point3,
        /// Principal direction for κ_max (unit tangent vector).
        dir_max: Point3,
    }

    unsafe extern "C++" {
        include!("occt_wrapper.h");

        /// Opaque wrapper around TopoDS_Shape.
        type OcctShape;

        /// Opaque vector of shapes for passing N shapes across FFI.
        type OcctShapeVec;

        /// Opaque container holding the BRepAlgoAPI history records
        /// (Modified/Generated/Deleted for faces and edges) plus the
        /// fused result shape. Records are materialized eagerly at
        /// construction time because the BRepAlgoAPI tracking maps are
        /// tied to the algorithm object's lifetime — once that's gone,
        /// the maps are gone too.
        type BooleanOpHistory;

        /// Opaque container holding the BRepPrimAPI sweep history records
        /// (Modified/Generated/Deleted for faces and edges) plus
        /// FirstShape/LastShape cap-face indices, plus the swept result
        /// shape. Single-parent variant of `BooleanOpHistory`. Records are
        /// materialized eagerly at construction time for the same lifetime
        /// reason as `BooleanOpHistory`.
        type SweepOpHistory;

        /// Opaque container holding the `BRepOffsetAPI_ThruSections` loft
        /// history records (Modified/Generated/Deleted for faces and edges)
        /// plus first/last-cap face indices, plus the lofted result shape.
        /// **Multi-parent** variant: `parent_index` in each record denotes
        /// the section index 0..N-1 across N profiles (not always 0 like
        /// `SweepOpHistory`). No diagnostic counters — those are
        /// revolve-synthesis-specific. Records are materialized eagerly at
        /// construction time for the same lifetime reason as the others.
        /// Task 2619 (v0.2 persistent-naming-v2 5b).
        type LoftOpHistory;

        // --- OcctShapeVec builder + reader ---
        fn new_shape_vec() -> UniquePtr<OcctShapeVec>;
        fn shape_vec_push(vec: Pin<&mut OcctShapeVec>, shape: &OcctShape);
        fn shape_vec_len(vec: &OcctShapeVec) -> usize;
        fn shape_vec_at(vec: &OcctShapeVec, idx: usize) -> Result<UniquePtr<OcctShape>>;

        // --- Foundation constants ---

        /// Return OCCT's `Precision::Confusion()` value (~1e-7).
        ///
        /// Test-fixture-style helper intentionally compiled across the cxx bridge
        /// in every build.  Cfg-gating cxx::bridge entries is awkward, so the
        /// symbol is visible to all callers of `crate::ffi::ffi` even though the
        /// only present-day call site is the crate-private test module in
        /// `lib.rs`, which pins `reify_types::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M`
        /// against the authoritative OCCT value at runtime.
        /// `Precision::Confusion()` is a `constexpr` literal in OCCT — cannot
        /// throw, no `Result` wrapper needed.
        fn precision_confusion() -> f64;

        // --- Primitive construction ---
        fn make_box(width: f64, height: f64, depth: f64) -> Result<UniquePtr<OcctShape>>;
        fn make_cylinder(radius: f64, height: f64) -> Result<UniquePtr<OcctShape>>;
        fn make_sphere(radius: f64) -> Result<UniquePtr<OcctShape>>;

        // --- Boolean operations ---
        fn boolean_fuse(left: &OcctShape, right: &OcctShape) -> Result<UniquePtr<OcctShape>>;
        fn boolean_cut(left: &OcctShape, right: &OcctShape) -> Result<UniquePtr<OcctShape>>;
        fn boolean_common(left: &OcctShape, right: &OcctShape) -> Result<UniquePtr<OcctShape>>;

        // --- BRepAlgoAPI_* history (v0.2 persistent-naming-v2, task 2590) ---

        /// Run `BRepAlgoAPI_Fuse` on `left` and `right`, eagerly capturing
        /// the per-parent face/edge Modified/Generated/Deleted records
        /// alongside the fused result shape.
        fn boolean_fuse_with_history(
            left: &OcctShape,
            right: &OcctShape,
        ) -> Result<UniquePtr<BooleanOpHistory>>;

        /// Move the result shape out of the history wrapper for
        /// registration in the kernel's shape table. Subsequent
        /// `_take_result_shape` calls return an empty pointer.
        fn boolean_op_history_take_result_shape(
            history: Pin<&mut BooleanOpHistory>,
        ) -> UniquePtr<OcctShape>;

        /// Modified records for parent faces (flat groups of 3 u32:
        /// `parent_index, parent_subshape_index, result_subshape_index`).
        fn boolean_op_history_face_modified(history: &BooleanOpHistory) -> Vec<u32>;
        /// Generated records for parent faces (flat groups of 3).
        fn boolean_op_history_face_generated(history: &BooleanOpHistory) -> Vec<u32>;
        /// Deleted records for parent faces (flat groups of 2 u32:
        /// `parent_index, parent_subshape_index`).
        fn boolean_op_history_face_deleted(history: &BooleanOpHistory) -> Vec<u32>;
        /// Modified records for parent edges (flat groups of 3).
        fn boolean_op_history_edge_modified(history: &BooleanOpHistory) -> Vec<u32>;
        /// Generated records for parent edges (flat groups of 3).
        fn boolean_op_history_edge_generated(history: &BooleanOpHistory) -> Vec<u32>;
        /// Deleted records for parent edges (flat groups of 2).
        fn boolean_op_history_edge_deleted(history: &BooleanOpHistory) -> Vec<u32>;
        /// Count of Modified/Generated children that could not be found in the
        /// result map and were silently skipped. Zero for a well-formed boolean.
        fn boolean_op_history_silent_drop_count(history: &BooleanOpHistory) -> u32;

        // --- BRepPrimAPI sweep history (v0.2 persistent-naming-v2, task 2573) ---

        /// Run `BRepPrimAPI_MakePrism` on `profile` along the direction
        /// `(dx, dy, dz)`, eagerly capturing the per-parent face/edge
        /// Modified/Generated/Deleted records and the FirstShape/LastShape
        /// cap-face indices alongside the swept result shape.
        fn make_prism_with_history(
            profile: &OcctShape,
            dx: f64,
            dy: f64,
            dz: f64,
        ) -> Result<UniquePtr<SweepOpHistory>>;

        /// Run `BRepPrimAPI_MakeRevol` on `profile` about the axis at
        /// origin `(ox, oy, oz)` with direction `(ax, ay, az)` for
        /// `angle_rad` radians, eagerly capturing the per-parent face/edge
        /// Modified/Generated/Deleted records and (for partial revolutions)
        /// the FirstShape/LastShape cap-face indices alongside the swept
        /// result shape. Under full revolution (FirstShape == LastShape)
        /// both cap-index lists are empty.
        fn make_revolve_with_history(
            profile: &OcctShape,
            ox: f64,
            oy: f64,
            oz: f64,
            ax: f64,
            ay: f64,
            az: f64,
            angle_rad: f64,
        ) -> Result<UniquePtr<SweepOpHistory>>;

        /// Run `BRepOffsetAPI_MakePipe` on `profile` along `spine` (a
        /// wire), eagerly capturing the per-parent face/edge/vertex
        /// Modified/Generated/Deleted records and the FirstShape/LastShape
        /// cap-face indices alongside the swept result shape. Reuses the
        /// SweepOpHistory shape (single-parent, `parent_index` always 0)
        /// since `BRepOffsetAPI_MakePipe` inherits the same
        /// Modified/Generated/IsDeleted/FirstShape/LastShape interface as
        /// prism / revolve via `BRepBuilderAPI_MakeShape`. Task 5b (#2619).
        fn make_pipe_with_history(
            profile: &OcctShape,
            spine: &OcctShape,
        ) -> Result<UniquePtr<SweepOpHistory>>;

        /// Move the result shape out of the sweep-history wrapper for
        /// registration in the kernel's shape table. Subsequent
        /// `_take_result_shape` calls return an empty pointer.
        fn sweep_op_history_take_result_shape(
            history: Pin<&mut SweepOpHistory>,
        ) -> UniquePtr<OcctShape>;

        /// Modified records for parent faces (flat groups of 3 u32:
        /// `parent_index, parent_subshape_index, result_subshape_index`).
        /// `parent_index` is always 0 for sweep ops (single parent profile).
        fn sweep_op_history_face_modified(history: &SweepOpHistory) -> Vec<u32>;
        /// Generated records for parent faces (flat groups of 3).
        fn sweep_op_history_face_generated(history: &SweepOpHistory) -> Vec<u32>;
        /// Deleted records for parent faces (flat groups of 2).
        fn sweep_op_history_face_deleted(history: &SweepOpHistory) -> Vec<u32>;
        /// Modified records for parent edges (flat groups of 3).
        fn sweep_op_history_edge_modified(history: &SweepOpHistory) -> Vec<u32>;
        /// Generated records for parent edges (flat groups of 3).
        fn sweep_op_history_edge_generated(history: &SweepOpHistory) -> Vec<u32>;
        /// Deleted records for parent edges (flat groups of 2).
        fn sweep_op_history_edge_deleted(history: &SweepOpHistory) -> Vec<u32>;
        /// 0-based result face_map indices of the FirstShape() (start) cap
        /// faces — exactly one for a single-face profile, possibly more for
        /// a compound profile, empty for a full-2π revolve.
        fn sweep_op_history_start_cap_face_indices(history: &SweepOpHistory) -> Vec<u32>;
        /// 0-based result face_map indices of the LastShape() (end) cap faces.
        fn sweep_op_history_end_cap_face_indices(history: &SweepOpHistory) -> Vec<u32>;
        /// Count of non-degenerate, untracked profile edges that did not produce a
        /// face_generated record during the full-revolution synthesis post-pass.
        /// Always 0 for prism ops and partial revolves; non-zero indicates a gap.
        fn sweep_op_history_unsynthesized_profile_edge_count(history: &SweepOpHistory) -> u32;
        /// Count of face_generated records dropped by the post-sort dedup pass
        /// because their parent_subshape_index duplicated the preceding record.
        /// Zero for a well-formed sweep.
        fn sweep_op_history_duplicate_parent_subshape_index_count(history: &SweepOpHistory) -> u32;
        /// Count of Modified/Generated children silently dropped because they could not
        /// be found in the result face/edge map. Zero for a well-formed sweep.
        fn sweep_op_history_silent_drop_count(history: &SweepOpHistory) -> u32;

        // --- BRepOffsetAPI_ThruSections loft history (task 2619, step-6) ---

        /// Run `BRepOffsetAPI_ThruSections` on `profiles` (>= 2 wire
        /// sections), eagerly capturing the per-section
        /// (multi-parent) face/edge correspondence records and the
        /// FirstShape/LastShape cap-face indices alongside the lofted
        /// result shape. `is_solid=true` produces a closed solid with
        /// non-empty cap lists; `is_solid=false` produces an open shell
        /// with empty cap lists. Errors out if `profiles.len() < 2` or
        /// the algorithm fails (`!IsDone()`).
        fn make_loft_with_history(
            profiles: &OcctShapeVec,
            is_solid: bool,
        ) -> Result<UniquePtr<LoftOpHistory>>;

        /// Move the result shape out of the loft-history wrapper for
        /// registration in the kernel's shape table. Subsequent
        /// `_take_result_shape` calls return an empty pointer.
        fn loft_op_history_take_result_shape(
            history: Pin<&mut LoftOpHistory>,
        ) -> UniquePtr<OcctShape>;

        /// Modified records for parent (section) faces (flat groups of 3 u32:
        /// `parent_index, parent_subshape_index, result_subshape_index`).
        /// `parent_index` is the section index 0..N-1. Expected to be empty
        /// for `BRepOffsetAPI_ThruSections` (the algorithm generates a
        /// fresh shape rather than transforming a parent), kept for layout
        /// uniformity with `sweep_op_history_face_modified`.
        fn loft_op_history_face_modified(history: &LoftOpHistory) -> Vec<u32>;
        /// Generated records for parent (section) edges (flat groups of 3).
        /// Each record corresponds to one section edge → one lateral
        /// result face mapped via `BRepOffsetAPI_ThruSections::GeneratedFace(edge)`.
        fn loft_op_history_face_generated(history: &LoftOpHistory) -> Vec<u32>;
        /// Deleted records for parent (section) faces (flat groups of 2).
        /// Expected to be empty; kept for layout uniformity.
        fn loft_op_history_face_deleted(history: &LoftOpHistory) -> Vec<u32>;
        /// Modified records for parent (section) edges. Expected to be empty.
        fn loft_op_history_edge_modified(history: &LoftOpHistory) -> Vec<u32>;
        /// Generated records for parent (section) edges. Expected to be empty
        /// (loft surfaces in 5b only emit face-level Generated records via
        /// `GeneratedFace`); kept for layout uniformity.
        fn loft_op_history_edge_generated(history: &LoftOpHistory) -> Vec<u32>;
        /// Deleted records for parent (section) edges. Expected to be empty.
        fn loft_op_history_edge_deleted(history: &LoftOpHistory) -> Vec<u32>;
        /// 0-based result face_map indices of the FirstShape() (start) cap
        /// faces. Populated only when constructed with `is_solid=true`.
        fn loft_op_history_start_cap_face_indices(history: &LoftOpHistory) -> Vec<u32>;
        /// 0-based result face_map indices of the LastShape() (end) cap faces.
        /// Populated only when constructed with `is_solid=true`.
        fn loft_op_history_end_cap_face_indices(history: &LoftOpHistory) -> Vec<u32>;

        /// Test fixture: run the post-sort/dedup pass on a synthetic flat
        /// `face_generated`-layout input. Returns the deduplicated triples and the
        /// count of dropped duplicates. Used to test dedup logic without real OCCT.
        fn revolve_synthesis_post_sort_for_test(input: &[u32]) -> RevolveSynthesisPostSortResult;

        /// Probe whether `a` and `b` are intersecting (non-positive minimum distance)
        /// via BRepExtrema_DistShapeShape. Returns true iff dist.Value() <= 0.0.
        /// Face-touching pairs count as intersecting.
        fn shapes_intersect(a: &OcctShape, b: &OcctShape) -> Result<bool>;

        // --- Local-feature op history (v0.2 persistent-naming-v2, task 2655) ---

        /// Opaque container holding the BRepFilletAPI_MakeFillet /
        /// BRepFilletAPI_MakeChamfer history records (Modified/Generated/Deleted
        /// for faces and edges) plus the modified result shape. Single-parent
        /// variant of `BooleanOpHistory` with no cap-face concept. Records are
        /// materialized eagerly at construction time because the algorithm's
        /// tracking maps are tied to its lifetime.
        type LocalFeatureOpHistory;

        /// Run `BRepFilletAPI_MakeFillet` on `shape` with the given `radius`
        /// applied to every edge, eagerly capturing the per-parent face/edge
        /// Modified/Generated/Deleted records alongside the modified result shape.
        fn make_fillet_with_history(
            shape: &OcctShape,
            radius: f64,
        ) -> Result<UniquePtr<LocalFeatureOpHistory>>;

        /// Run `BRepFilletAPI_MakeChamfer` on `shape` with the given `distance`
        /// applied to every edge, eagerly capturing the per-parent face/edge
        /// Modified/Generated/Deleted records alongside the modified result shape.
        fn make_chamfer_with_history(
            shape: &OcctShape,
            distance: f64,
        ) -> Result<UniquePtr<LocalFeatureOpHistory>>;

        /// Move the result shape out of the local-feature-history wrapper for
        /// registration in the kernel's shape table.
        fn local_feature_op_history_take_result_shape(
            history: Pin<&mut LocalFeatureOpHistory>,
        ) -> UniquePtr<OcctShape>;

        /// Modified records for parent faces (flat groups of 3 u32:
        /// `parent_index, parent_subshape_index, result_subshape_index`).
        /// `parent_index` is always 0 (single parent).
        fn local_feature_op_history_face_modified(history: &LocalFeatureOpHistory) -> Vec<u32>;
        /// Generated records for parent faces (flat groups of 3).
        fn local_feature_op_history_face_generated(history: &LocalFeatureOpHistory) -> Vec<u32>;
        /// Deleted records for parent faces (flat groups of 2 u32:
        /// `parent_index, parent_subshape_index`).
        fn local_feature_op_history_face_deleted(history: &LocalFeatureOpHistory) -> Vec<u32>;
        /// Modified records for parent edges (flat groups of 3).
        fn local_feature_op_history_edge_modified(history: &LocalFeatureOpHistory) -> Vec<u32>;
        /// Generated records for parent edges (flat groups of 3).
        fn local_feature_op_history_edge_generated(history: &LocalFeatureOpHistory) -> Vec<u32>;
        /// Deleted records for parent edges (flat groups of 2).
        fn local_feature_op_history_edge_deleted(history: &LocalFeatureOpHistory) -> Vec<u32>;
        /// Count of Modified/Generated children silently dropped because they could
        /// not be found in the result map. Zero for a well-formed fillet/chamfer.
        fn local_feature_op_history_silent_drop_count(history: &LocalFeatureOpHistory) -> u32;

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

        /// Apply a general non-rigid affine transform (3×3 linear + translation)
        /// to `shape` using OCCT's `gp_GTrsf` / `BRepBuilderAPI_GTransform`.
        ///
        /// The 3×3 linear part is given in row-major order `(m00..m22)` and the
        /// translation column is `(tx, ty, tz)`.  These map to OCCT's 1-indexed
        /// row-major 3×4 `SetValues(a11, a12, a13, a14,  a21, a22, a23, a24,  a31, a32, a33, a34)`
        /// as `m00→a11, m01→a12, m02→a13, tx→a14, m10→a21, …`.
        ///
        /// The operation runs with `Copy=true`, so the source shape is never mutated;
        /// a fresh `UniquePtr<OcctShape>` is returned.
        ///
        /// Singular-input guard: rejects rank-deficient linear parts using a scale-invariant
        /// Hadamard-ratio check (`|det| / (‖row0‖·‖row1‖·‖row2‖) < 1e-12`), with an error
        /// message containing "singular". Non-uniform scale and shear are valid (e.g.
        /// `diag(1,1,2)` → ratio=1.0; `diag(1e-5,1e-5,1e-5)` → ratio=1.0 ≫ 1e-12).
        /// Per PRD `docs/prds/v0_6/affine-map-type.md` §5 task ε.
        ///
        /// # Errors
        /// Returns an error if the linear part is singular, or if
        /// `BRepBuilderAPI_GTransform` fails (`IsDone()` is false).
        fn gtransform_shape(
            shape: &OcctShape,
            m00: f64,
            m01: f64,
            m02: f64,
            m10: f64,
            m11: f64,
            m12: f64,
            m20: f64,
            m21: f64,
            m22: f64,
            tx: f64,
            ty: f64,
            tz: f64,
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
            cx: f64,
            cy: f64,
            cz: f64,
            radius: f64,
            start_angle: f64,
            end_angle: f64,
            ax: f64,
            ay: f64,
            az: f64,
        ) -> Result<UniquePtr<OcctShape>>;
        fn make_helix_wire(radius: f64, pitch: f64, height: f64) -> Result<UniquePtr<OcctShape>>;
        /// Build a polyline wire from N >= 2 points (flat 3*N coord slice).
        /// Produces N-1 line edges.  Stable kernel FFI primitive: polygon-face
        /// backing wires, multi-segment sweep/pipe paths, and
        /// BRepAdaptor_CompCurve composite-wire testing.
        fn make_polyline_wire(coords: &[f64], n_points: usize) -> Result<UniquePtr<OcctShape>>;
        fn make_interp_curve(coords: &[f64], n_points: usize) -> Result<UniquePtr<OcctShape>>;
        fn make_bezier_curve(coords: &[f64], n_points: usize) -> Result<UniquePtr<OcctShape>>;
        fn make_nurbs_curve(
            pole_coords: &[f64],
            n_poles: usize,
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
        fn make_triangle_face(
            x1: f64,
            z1: f64,
            x2: f64,
            z2: f64,
            x3: f64,
            z3: f64,
            cy: f64,
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
        /// Angle (radians) between the outward normals of two `TopoDS_Face` shapes,
        /// sampled at each face's surface centroid.
        ///
        /// Note: this is the angle between outward normals, not a classical
        /// dihedral angle (which requires a shared edge).
        ///
        /// Algorithm: `acos(clamp(n_a · n_b, -1, 1))` where each `n` is the
        /// face's unit outward normal at its centroid. Honours `TopAbs_REVERSED`
        /// orientation. Returns radians in `[0, π]`.
        ///
        /// Throws (surfaces as `Err`) if either shape is not a face, has no
        /// underlying surface, or yields a degenerate normal.
        fn surface_angle(face_a: &OcctShape, face_b: &OcctShape) -> Result<f64>;

        /// Unit outward normal at the parametric point `(u, v)` on `face`.
        ///
        /// The shape must be a `TopoDS_Face`. Algorithm: `BRepAdaptor_Surface::D1`
        /// → `Du × Dv` → magnitude check → `TopAbs_REVERSED` orientation flip →
        /// normalize. Returns the normalized outward unit vector.
        ///
        /// Throws (surfaces as `Err`) if the shape is not a face, has no underlying
        /// surface, or yields a degenerate (zero-magnitude) cross product at `(u, v)`.
        fn surface_normal_at(face: &OcctShape, u: f64, v: f64) -> Result<Point3>;

        /// Outward unit normal of a face at the Cartesian world-space point
        /// `(px, py, pz)` (metres).
        ///
        /// Projects the query point via `ShapeAnalysis_Surface::ValueOfUV(p, 1e-9)`
        /// then delegates to `face_outward_unit_normal_at_uv` — the same
        /// orientation-aware helper as `query_face_normal` and `surface_normal_at`.
        ///
        /// Throws (surfaces as `Err`) if the shape is not a face, has no underlying
        /// surface, or yields a degenerate (zero-magnitude) normal at the projected
        /// `(u, v)`.
        fn surface_normal_at_point(face: &OcctShape, px: f64, py: f64, pz: f64) -> Result<Point3>;

        /// Gaussian, mean, and principal curvatures at the parametric point
        /// `(u, v)` on `face`, plus unit-length principal-direction tangents.
        ///
        /// Uses `BRepAdaptor_Surface::D2` — same abstraction as `surface_normal_at`
        /// — so both APIs agree in world frame on faces with non-identity
        /// `TopoLoc_Location`. Curvature is derived from the first/second
        /// fundamental forms with an orientation-aware outward normal; sign
        /// convention for H and κ follows that normal (correct for both FORWARD
        /// and REVERSED faces without a post-hoc swap). Gaussian K is invariant.
        ///
        /// Throws (surfaces as `Err`) if the shape is not a face, has no
        /// underlying surface, or curvature is undefined at `(u, v)`.
        fn curvature_at(face: &OcctShape, u: f64, v: f64) -> Result<CurvatureProps>;

        /// Signed curvature of an edge at the closest point on the curve to the
        /// world-space query point `(px, py, pz)`.
        ///
        /// Projects the query point onto the edge's underlying `Geom_Curve` via
        /// `GeomAPI_ProjectPointOnCurve`, then evaluates curvature via
        /// `BRepLProp_CLProps`. Sign follows the Frenet frame (positive toward
        /// principal normal).
        ///
        /// Throws (surfaced as `Err`) if the shape is not an edge, the edge is
        /// degenerate (no underlying curve), projection fails, or the tangent is
        /// undefined at the projected parameter.
        fn curve_curvature_at(edge: &OcctShape, px: f64, py: f64, pz: f64) -> Result<f64>;
        fn query_centroid(shape: &OcctShape) -> Result<Point3>;
        /// Surface-properties centroid for a 2D sub-shape (TopoDS_Face).
        /// Used by the `Centroid` query path when the stored repr is
        /// `BRepKind::Face`, since `query_centroid` (volume-based) returns
        /// the origin for isolated faces with zero enclosed volume.
        fn query_face_centroid(shape: &OcctShape) -> Result<Point3>;
        fn query_bbox(shape: &OcctShape) -> Result<BBox>;

        fn query_distance(shape1: &OcctShape, shape2: &OcctShape) -> Result<f64>;

        /// Minimum BREP distance between `a` and `b` via BRepExtrema_DistShapeShape.
        /// Separate symbol from query_distance for the kinematic-constraints call
        /// site (task 2531; see PRD task 7).
        fn min_clearance(a: &OcctShape, b: &OcctShape) -> Result<f64>;

        /// Minimum BREP distance between `a` and `b` after pre-composing `t_rel`
        /// into the cheaper-by-topology side (PRD §6.2 + §9.2, task 3841).
        ///
        /// The transformed copy uses `BRepBuilderAPI_Transform(…, Standard_False)`
        /// (TopLoc_Location encoding — no geometry bake, no PNv2 concerns).
        fn distance_with_transform(
            a: &OcctShape,
            b: &OcctShape,
            t_rel: &Transform3Props,
        ) -> Result<f64>;

        /// Probe whether `a` and `b` interfere after pre-composing `t_rel` into
        /// the cheaper-by-topology side (PRD §6.2 + §9.2, task 3841).
        ///
        /// Returns true iff the minimum BREP distance after transform is ≤ 0.0.
        /// Face-touching pairs count as interfering — matches `shapes_intersect` semantics.
        fn interferes_with_transform(
            a: &OcctShape,
            b: &OcctShape,
            t_rel: &Transform3Props,
        ) -> Result<bool>;

        /// Return the closest point on `shape` to the query point (px, py, pz).
        ///
        /// Uses `BRepExtrema_DistShapeShape(shape, vertex)` where the vertex is
        /// built from the query point. Returns `PointOnShape1(1)` — the witness on
        /// the input shape. Operand ordering matches `min_clearance` / `query_distance`.
        fn closest_point_on_shape(shape: &OcctShape, px: f64, py: f64, pz: f64) -> Result<Point3>;

        /// Return the geometric position of `shape` (must be a `TopoDS_Vertex`)
        /// via `BRep_Tool::Pnt`. Mandated by PRD `mesh-morphing-phase-2.md`
        /// §3.4 `vertex_position`: snap to exact coordinates, no closest-point
        /// computation. Errors if `shape.ShapeType() != TopAbs_VERTEX`.
        fn vertex_point(shape: &OcctShape) -> Result<Point3>;

        /// Test whether the query point `(px, py, pz)` lies on the BREP boundary
        /// (face/edge/vertex) of `shape` within `tolerance`.
        ///
        /// Uses `BRepExtrema_DistShapeShape(shape, vertex)` where the vertex is built
        /// from the query point, returning `dist.Value() <= tolerance`. Operand ordering
        /// mirrors `closest_point_on_shape` and `min_clearance`.
        ///
        /// **Interior solid points return `true`:** `BRepExtrema_DistShapeShape` treats
        /// an interior query vertex as overlapping the solid and reports `dist.Value() = 0`,
        /// so `point_on_shape` returns `true` for any interior `TopoDS_Solid` point at any
        /// positive tolerance. The primitive cannot distinguish on-surface from inside-solid
        /// for solids; see the C++ header for the full contract and the
        /// `BRepClass3d_SolidClassifier` pre-filter escape hatch.
        ///
        /// Callers commonly pass `Precision::Confusion()` (~1e-7) for `tolerance`.
        ///
        /// **Tolerance precondition:** `tolerance` must be a non-negative finite `f64`.
        /// Negative or NaN values cause the C++ implementation to throw a
        /// `std::runtime_error`, which maps to `Err(QueryError::QueryFailed(_))` at the
        /// Rust call site rather than silently returning a misleading `false`.
        fn point_on_shape(
            shape: &OcctShape,
            px: f64,
            py: f64,
            pz: f64,
            tolerance: f64,
        ) -> Result<bool>;

        /// Test whether `(px, py, pz)` is inside or on the boundary of a closed solid.
        ///
        /// Wraps `BRepClass3d_SolidClassifier(shape).Perform(gp_Pnt, tolerance)`;
        /// returns `true` when `State() == TopAbs_IN || State() == TopAbs_ON`.
        ///
        /// **Tolerance precondition:** `tolerance` must be a non-negative finite `f64`.
        /// Negative or NaN values cause the C++ implementation to throw, which maps to
        /// `Err(QueryError::QueryFailed(_))` at the Rust call site.
        fn contains_solid(
            shape: &OcctShape,
            px: f64,
            py: f64,
            pz: f64,
            tolerance: f64,
        ) -> Result<bool>;

        /// Test whether two shapes are geometrically equivalent within `tolerance`
        /// by topology-count matching and sampled-vertex proximity.
        ///
        /// STRICT-VARIANT NOTE: This is the asymmetric sampled-point geo_equiv
        /// (PRD §5.1, KGQ-δ).  A future `geo_equiv_strict` using symmetric
        /// Hausdorff distance is deferred to v0.4 per PRD §5.1 + Open Question §10.
        ///
        /// **Tolerance precondition:** `tolerance` must be a non-negative finite `f64`.
        /// Negative or NaN values cause the C++ implementation to throw, which maps to
        /// `Err(QueryError::QueryFailed(_))` at the Rust call site.
        fn geo_equiv_topo_sample(
            a: &OcctShape,
            b: &OcctShape,
            tolerance: f64,
            sample_count: usize,
        ) -> Result<bool>;

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

        /// Faces that own the edge at `edge_index` (0-based, TopExp order
        /// for both inputs and outputs). For a manifold solid every edge has
        /// exactly two ancestor faces; degenerate / non-manifold edges may
        /// surface 1 or > 2. Deduplicated; returned ascending. Errors if
        /// `edge_index` is out of range.
        fn ancestor_faces_of_edge(shape: &OcctShape, edge_index: u32) -> Result<Vec<u32>>;

        /// Classify the underlying surface of a face by its OCCT
        /// `BRepAdaptor_Surface::GetType()` (`GeomAbs_*`) result. Returns a
        /// canonical surface-kind name string (`"Plane"` / `"Cylinder"` /
        /// `"Cone"` / `"Sphere"` / `"Torus"` / `"BezierSurface"` /
        /// `"BSplineSurface"` / `"OffsetSurface"` / `"Other"`) decoded by
        /// `reify_types::FaceSurfaceKind::try_from_str` on the Rust side.
        /// Errors if `shape` is not a `TopAbs_FACE`.
        fn face_surface_kind(shape: &OcctShape) -> Result<String>;

        /// Classify the underlying curve of an edge by its OCCT
        /// `BRepAdaptor_Curve::GetType()` (`GeomAbs_*`) result. Returns a
        /// canonical curve-kind name string (`"Line"` / `"Circle"` /
        /// `"Ellipse"` / `"Hyperbola"` / `"Parabola"` / `"BezierCurve"` /
        /// `"BSplineCurve"` / `"OffsetCurve"` / `"Other"`) decoded by
        /// `reify_types::EdgeCurveKind::try_from_str` on the Rust side.
        /// Errors if `shape` is not a `TopAbs_EDGE`.
        fn edge_curve_kind(shape: &OcctShape) -> Result<String>;

        /// Materialize the unique edges of `shape` into an OcctShapeVec
        /// (canonical TopExp::MapShapes order, deduplicated by IsSame).
        fn get_edges(shape: &OcctShape) -> Result<UniquePtr<OcctShapeVec>>;

        /// Materialize the unique faces of `shape` into an OcctShapeVec
        /// (canonical TopExp::MapShapes order, deduplicated by IsSame).
        fn get_faces(shape: &OcctShape) -> Result<UniquePtr<OcctShapeVec>>;

        /// Materialize the unique vertices of `shape` into an OcctShapeVec
        /// (canonical `TopExp::MapShapes(.., TopAbs_VERTEX, ..)` order,
        /// deduplicated by IsSame). Built per call (no `vertex_map()` cache).
        fn get_vertices(shape: &OcctShape) -> Result<UniquePtr<OcctShapeVec>>;

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

        /// Single vertex at (x, y, z) → TopAbs_VERTEX. Parameterised companion
        /// to `make_vertex_for_test` for tests that need a pinned non-origin
        /// location (e.g. `vertex_point` round-trip verification).
        fn make_vertex_at_for_test(
            x: f64,
            y: f64,
            z: f64,
        ) -> Result<UniquePtr<OcctShape>>;

        /// CompSolid wrapping one 10×10×10 mm box → TopAbs_COMPSOLID; type-guard passes.
        fn make_compsolid_for_test() -> Result<UniquePtr<OcctShape>>;

        /// Apply rotation+translation using `BRepBuilderAPI_Transform(..., Copy=false)`,
        /// encoding the transform into `TopLoc_Location` rather than baking it into
        /// geometry. Used by placed-face integration tests to exercise the non-identity
        /// location path through `BRepAdaptor_Surface`. See C++ header for full contract.
        fn apply_test_placement_for_test(
            shape: &OcctShape,
            ax: f64,
            ay: f64,
            az: f64,
            angle_rad: f64,
            dx: f64,
            dy: f64,
            dz: f64,
        ) -> Result<UniquePtr<OcctShape>>;

        // --- Export ---
        fn export_step(shape: &OcctShape) -> Result<String>;

        // --- BRep serialization ---
        fn serialize_brep(shape: &OcctShape) -> Result<String>;
        fn deserialize_brep(data: &CxxString) -> Result<UniquePtr<OcctShape>>;

        // --- Tessellation ---
        fn tessellate_shape(shape: &OcctShape, tolerance: f64) -> Result<TessResult>;

    }
}
