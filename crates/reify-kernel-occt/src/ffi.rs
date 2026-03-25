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

    unsafe extern "C++" {
        include!("occt_wrapper.h");

        /// Opaque wrapper around TopoDS_Shape.
        type OcctShape;

        // --- Primitive construction ---
        fn make_box(width: f64, height: f64, depth: f64) -> Result<UniquePtr<OcctShape>>;
        fn make_cylinder(radius: f64, height: f64) -> Result<UniquePtr<OcctShape>>;
        fn make_sphere(radius: f64) -> Result<UniquePtr<OcctShape>>;

        // --- Boolean operations ---
        fn boolean_fuse(left: &OcctShape, right: &OcctShape) -> Result<UniquePtr<OcctShape>>;
        fn boolean_cut(left: &OcctShape, right: &OcctShape) -> Result<UniquePtr<OcctShape>>;
        fn boolean_common(left: &OcctShape, right: &OcctShape) -> Result<UniquePtr<OcctShape>>;

        // --- Modifications ---
        fn fillet_all_edges(shape: &OcctShape, radius: f64) -> Result<UniquePtr<OcctShape>>;

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

        // --- Draft ---
        fn draft_shape(
            shape: &OcctShape,
            angle_rad: f64,
            plane_shape: &OcctShape,
        ) -> Result<UniquePtr<OcctShape>>;

        // --- Thicken / Shell ---
        fn thicken_shape(
            shape: &OcctShape,
            offset: f64,
        ) -> Result<UniquePtr<OcctShape>>;
        fn shell_shape(
            shape: &OcctShape,
            thickness: f64,
            face_indices: &Vec<u32>,
        ) -> Result<UniquePtr<OcctShape>>;

        // --- Wire helpers / Loft ---
        fn make_circle_wire(radius: f64, z_height: f64) -> Result<UniquePtr<OcctShape>>;
        fn make_circle_face(radius: f64, z_height: f64) -> Result<UniquePtr<OcctShape>>;
        fn loft_two_profiles(
            wire1: &OcctShape,
            wire2: &OcctShape,
        ) -> Result<UniquePtr<OcctShape>>;
        fn loft_three_profiles(
            wire1: &OcctShape,
            wire2: &OcctShape,
            wire3: &OcctShape,
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

        // --- Queries ---
        fn query_volume(shape: &OcctShape) -> Result<f64>;
        fn query_area(shape: &OcctShape) -> Result<f64>;
        fn query_centroid(shape: &OcctShape) -> Result<Point3>;
        fn query_bbox(shape: &OcctShape) -> Result<BBox>;

        fn query_distance(shape1: &OcctShape, shape2: &OcctShape) -> Result<f64>;
        fn query_moment_of_inertia(
            shape: &OcctShape,
            ax: f64,
            ay: f64,
            az: f64,
        ) -> Result<f64>;

        // --- Export ---
        fn export_step(shape: &OcctShape) -> Result<String>;

        // --- BRep serialization ---
        fn serialize_brep(shape: &OcctShape) -> Result<String>;
        fn deserialize_brep(data: &CxxString) -> Result<UniquePtr<OcctShape>>;

        // --- Tessellation ---
        fn tessellate_shape(shape: &OcctShape, tolerance: f64) -> Result<TessResult>;
    }
}
