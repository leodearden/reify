use reify_build_utils::NativeDep;

fn main() {
    // reify-solver-elastic depends on reify-kernel-gmsh directly. The kernel
    // adapter's build.rs emits `rustc-link-arg=-Wl,-rpath,<gmsh_lib_dir>`,
    // but that directive does not propagate across package boundaries — so
    // this package's lib-unittests and integration test binaries launch with
    // empty RUNPATH and fail with `libgmsh.so.4.15: cannot open shared object
    // file` once `/etc/ld.so.conf.d/reify-deps.conf` is removed.
    //
    // Workspace binaries (`reify`, `reify-gui`) that transitively pull this
    // crate get RUNPATH via their own build.rs calls to
    // `emit_rpath_for_bins(NativeDep::Gmsh)`; this build.rs covers the
    // test-binary side.
    reify_build_utils::emit_rpath_for_tests(NativeDep::Gmsh);
}
