//! CXX bridge to the OCCT C++ wrapper.

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

        // --- Queries ---
        fn query_volume(shape: &OcctShape) -> Result<f64>;
        fn query_area(shape: &OcctShape) -> Result<f64>;
        fn query_centroid(shape: &OcctShape) -> Result<Point3>;
        fn query_bbox(shape: &OcctShape) -> Result<BBox>;

        // --- Export ---
        fn export_step(shape: &OcctShape) -> Result<String>;

        // --- Tessellation ---
        fn tessellate_shape(shape: &OcctShape, tolerance: f64) -> Result<TessResult>;
    }
}
