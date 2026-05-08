//! CXX bridge to the OpenVDB C++ wrapper.
//!
//! Gated on `cfg(has_openvdb)` — only compiled when `/opt/reify-deps` (or an
//! override via `OPENVDB_INCLUDE_DIR`/`OPENVDB_LIB_DIR`) is detected by
//! `build.rs`. When the cfg is not set, the stub kernel in `kernel.rs` is the
//! only `OpenVdbKernel` implementation, and this module is absent.
//!
//! # Namespace
//!
//! All C++ functions live in the `reify_openvdb` namespace (defined in
//! `cpp/openvdb_wrapper.h`). The cxx bridge maps them to Rust items in
//! `crate::ffi`.

#[allow(clippy::module_inception)]
#[cxx::bridge(namespace = "reify_openvdb")]
pub mod ffi {
    // Opaque handle wrapping openvdb::FloatGrid::Ptr. All ownership transfer
    // goes through std::unique_ptr<OpenVdbGridHandle>. The Rust kernel stores
    // cxx::UniquePtr<OpenVdbGridHandle> keyed by GeometryHandleId.
    unsafe extern "C++" {
        include!("openvdb_wrapper.h");

        type OpenVdbGridHandle;

        // Library lifecycle
        fn openvdb_initialize();
        fn openvdb_version_string() -> String;

        // Mesh → Volume: triangle-soup → narrow-band FloatGrid SDF.
        // Throws std::runtime_error on degenerate/empty mesh.
        fn mesh_to_volume_ffi(
            verts: &[[f32; 3]],
            tris: &[[u32; 3]],
            voxel_size: f64,
            half_width_voxels: f64,
        ) -> Result<UniquePtr<OpenVdbGridHandle>>;

        // Grid queries
        fn grid_active_voxel_count(h: &OpenVdbGridHandle) -> usize;
        fn grid_sample_sdf(h: &OpenVdbGridHandle, x: f64, y: f64, z: f64) -> f64;

        // Grid metadata accessors
        fn grid_bbox_min(h: &OpenVdbGridHandle) -> [f64; 3];
        fn grid_bbox_max(h: &OpenVdbGridHandle) -> [f64; 3];
        // Per-axis voxel sizes (X, Y, Z) — returns the diagonal of the
        // grid's linear transform. Anisotropic grids produce three distinct
        // values; the Rust caller propagates them into `SampledField.spacing`.
        fn grid_voxel_sizes(h: &OpenVdbGridHandle) -> [f64; 3];
        fn grid_units(h: &OpenVdbGridHandle) -> String;

        // File I/O — throw std::runtime_error on failure; cxx maps to Err.
        fn write_vdb_grid_ffi(
            h: &OpenVdbGridHandle,
            path: &str,
            grid_name: &str,
        ) -> Result<()>;
        fn read_vdb_grid_ffi(
            path: &str,
            grid_name: &str,
        ) -> Result<UniquePtr<OpenVdbGridHandle>>;

        // Densify active voxels into flat row-major f32 buffer (axis-0 = X
        // outermost). The Rust caller derives per-axis dimensions from the
        // bbox + voxel_sizes; `lower_to_sampled` cross-checks the buffer
        // length against the product of axis-grid lengths.
        fn grid_densify_to_buffer(h: &OpenVdbGridHandle) -> Vec<f32>;
    }
}
