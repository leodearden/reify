//! The test-fixtures TBB single-thread scope that run-twice determinism tests
//! hold (implicit-solids D-13 / B17).
//!
//! A binary of its own because `tbb::global_control` is process-global;
//! `SERIAL` keeps the restore assertion from observing the other test's scope.
//! Structure follows the `cfg(has_openvdb)` real-test / skip-stub pattern of
//! `tests/ingest_mesh_densify_tests.rs`.

#[cfg(has_openvdb)]
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(has_openvdb)]
fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(has_openvdb)]
#[test]
fn single_thread_scope_limits_and_restores_tbb_parallelism() {
    use reify_kernel_openvdb::tbb_scope::{TbbSingleThreadScope, tbb_max_allowed_parallelism};

    let _serial = serial();
    let p0 = tbb_max_allowed_parallelism();
    assert!(p0 >= 1, "TBB must report at least one thread; got {p0}");

    let scope = TbbSingleThreadScope::enter();
    assert_eq!(
        tbb_max_allowed_parallelism(),
        1,
        "inside the scope TBB must run single-threaded",
    );
    drop(scope);
    assert_eq!(
        tbb_max_allowed_parallelism(),
        p0,
        "dropping the scope must restore the prior parallelism",
    );
}

/// The kernel-level shape of B17: two FRESH kernels (not a cache hit) ingest
/// and tessellate the same grid under one scope, bit for bit.
#[cfg(has_openvdb)]
#[test]
fn fresh_kernels_tessellate_bitwise_identically_under_single_thread_scope() {
    use reify_ir::{GeometryKernel, ImplicitOptions, Mesh};
    use reify_kernel_openvdb::OpenVdbKernel;
    use reify_kernel_openvdb::tbb_scope::TbbSingleThreadScope;
    use reify_kernel_openvdb::test_fixtures::sphere_sdf_grid;

    let _serial = serial();
    let h = 5e-4;
    let band = 3;
    let grid = sphere_sdf_grid(
        [0.010, -0.020, 0.005],
        [1.0e-3, -1.5e-3, 2.0e-3],
        3e-3,
        [30, 34, 38],
        h,
    );
    let opts = ImplicitOptions::new(h, band).expect("valid options");

    let _scope = TbbSingleThreadScope::enter();
    let tessellate_on_a_fresh_kernel = || -> Mesh {
        let mut kernel = OpenVdbKernel::new();
        let handle = kernel
            .ingest_dense_sdf(&grid, &opts)
            .expect("ingest_dense_sdf");
        kernel.tessellate(handle.id, 0.0).expect("tessellate")
    };
    let first = tessellate_on_a_fresh_kernel();
    let second = tessellate_on_a_fresh_kernel();

    assert!(
        !first.vertices.is_empty() && !first.indices.is_empty(),
        "the sphere must surface, or the equality below is vacuous",
    );
    let vertex_bits = |m: &Mesh| m.vertices.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
    assert_eq!(
        vertex_bits(&first),
        vertex_bits(&second),
        "vertices must be bitwise identical"
    );
    assert_eq!(first.indices, second.indices, "indices must be identical");
}

#[cfg(not(has_openvdb))]
#[test]
fn tbb_single_thread_scope_skipped_without_cfg() {
    println!("tbb_single_thread_scope_tests: has_openvdb cfg not set, skipping TBB scope tests");
}
