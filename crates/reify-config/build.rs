use reify_build_utils::NativeDep;

fn main() {
    // `reify-config` itself has no native deps — but its
    // `tests/kernel_name_consistency.rs` dev-deps on all kernel adapters for a
    // cross-crate consistency check, so the test binary transitively links
    // libTKernel, libgmsh, libopenvdb. Cargo's `rustc-link-arg` directives in
    // the kernel adapter build.rs scripts do not propagate across package
    // boundaries, so without these emit_rpath_for_tests calls the test binary
    // launches with empty RUNPATH and relies on the system ld.so cache (i.e.
    // `/etc/ld.so.conf.d/reify-deps.conf`). Once that file is removed the
    // test binary fails at startup with
    // `libopenvdb.so.13.0: cannot open shared object file`.
    reify_build_utils::emit_rpath_for_tests(NativeDep::Occt);
    reify_build_utils::emit_rpath_for_tests(NativeDep::Gmsh);
    reify_build_utils::emit_rpath_for_tests(NativeDep::OpenVdb);
}
