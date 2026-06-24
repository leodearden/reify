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

        // Volume → Mesh (marching cubes): FloatGrid SDF → triangle soup.
        //
        // `iso_level`: isovalue (0.0 = zero level-set / SDF surface).
        // `adaptivity`: in [0, 1]; 0.0 = uniform MC, 1.0 = max adaptive.
        //
        // Quads from volumeToMesh are triangulated into two triangles each:
        //   (i, j, k) and (i, k, l) for quad (i, j, k, l).
        //
        // Single call — marching cubes runs exactly once.
        // out_vertices and out_indices are appended in-place (caller supplies
        // Vec<f32>/Vec<u32> by &mut reference); avoids a shared-struct
        // redefinition conflict between the user header and cxx bridge output.
        fn volume_to_mesh_ffi(
            h: &OpenVdbGridHandle,
            iso_level: f64,
            adaptivity: f64,
            out_vertices: &mut Vec<f32>,
            out_indices: &mut Vec<u32>,
        );

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
        // Pure read of the cached MetaMap-backed grid name. No lazy init,
        // no tree walk — safe to call from `&self` callers under the Sync
        // audit list at `src/kernel_real.rs:220-260`. Used by the
        // test-only `OpenVdbKernel::grid_name_for_test` accessor to pin
        // the no-mutation invariant for `write_vdb_grid` (regression
        // guard against in-place `setName` reverts).
        fn grid_name(h: &OpenVdbGridHandle) -> String;

        // File I/O — throw std::runtime_error on failure; cxx maps to Err.
        fn write_vdb_grid_ffi(h: &OpenVdbGridHandle, path: &str, grid_name: &str) -> Result<()>;
        fn read_vdb_grid_ffi(path: &str, grid_name: &str) -> Result<UniquePtr<OpenVdbGridHandle>>;

        // Densify active voxels into flat row-major f32 buffer (axis-0 = X
        // outermost). The Rust caller derives per-axis dimensions from the
        // bbox + voxel_sizes; `lower_to_sampled` cross-checks the buffer
        // length against the product of axis-grid lengths.
        //
        // Throws `std::runtime_error` (mapped to `Err(cxx::Exception)`) if
        // the active bbox would exceed `GRID_DENSIFY_MAX_VOXELS`
        // (~256M voxels ≈ 1 GiB at 4 bytes/float) so a malformed or
        // adversarially-large .vdb cannot OOM the host process. The Rust
        // caller in `read_vdb_file` surfaces the exception as
        // `IngestError::FileReadError { detail: "grid too large: …" }`.
        fn grid_densify_to_buffer(h: &OpenVdbGridHandle) -> Result<Vec<f32>>;
    }
}
