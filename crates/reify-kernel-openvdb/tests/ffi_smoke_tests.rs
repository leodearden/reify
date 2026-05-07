//! Smoke tests for the OpenVDB cxx-bridge FFI layer.
//!
//! Two test arms: one for `cfg(has_openvdb)` builds (exercises the real FFI)
//! and one for `cfg(not(has_openvdb))` builds (trivial skip that keeps the
//! test count stable across both build modes).

/// `cfg(has_openvdb)` arm — calls `reify_kernel_openvdb::ffi::openvdb_version_string()`
/// and asserts the returned string starts with "13." (matching libopenvdb 13.0.0
/// from the conda-forge env at /opt/reify-deps).
///
/// This test is RED until step-2 (build wiring + cxx-bridge + cpp wrapper) lands.
#[cfg(has_openvdb)]
#[test]
fn openvdb_version_is_13() {
    let version = reify_kernel_openvdb::ffi::openvdb_version_string();
    assert!(
        version.starts_with("13."),
        "Expected libopenvdb 13.x from conda-forge env; got {version:?}"
    );
}

/// `cfg(not(has_openvdb))` arm — trivial pass that keeps the test count stable
/// when building without `/opt/reify-deps` (no OpenVDB available).
#[cfg(not(has_openvdb))]
#[test]
fn openvdb_smoke_skipped_without_cfg() {
    // OpenVDB not available in this build; FFI smoke test skipped.
    println!("openvdb_smoke_skipped_without_cfg: has_openvdb cfg not set, skip");
    assert!(true);
}
