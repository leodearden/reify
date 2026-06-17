use reify_build_utils::NativeDep;

fn main() {
    // Embed RUNPATH into the `reify` binary for every native dep this binary
    // transitively links. `cargo:rustc-link-arg-bins` is required (not just
    // `rustc-link-arg`) because Cargo does NOT propagate `rustc-link-arg`
    // directives across package boundaries — the directives in the kernel
    // adapters' build.rs scripts only apply to bins/tests in their own
    // package. Without this, the workspace `reify` binary would launch with
    // an empty RUNPATH and rely on the system ld.so cache (which is exactly
    // what /etc/ld.so.conf.d/reify-deps.conf provides — and what we want to
    // remove).
    //
    // `reify-cli` transitively pulls:
    //   - OCCT via `reify-kernel-occt` (direct dep)
    //   - Gmsh via `reify-eval → reify-solver-elastic → reify-kernel-gmsh`
    //   - OpenVDB via `reify-eval → reify-kernel-openvdb` (since task 3576)

    // Declare has_openvdb as a known cfg so rustc does not warn on unknown cfgs.
    println!("cargo::rustc-check-cfg=cfg(has_openvdb)");
    // Enable has_openvdb if OpenVDB native libraries are available.
    if reify_build_utils::find(reify_build_utils::NativeDep::OpenVdb).is_some() {
        println!("cargo:rustc-cfg=has_openvdb");
    }
    // Emit RPATH for test binaries that transitively link libopenvdb.
    reify_build_utils::emit_rpath_for_tests(NativeDep::OpenVdb);

    reify_build_utils::emit_rpath_for_bins(NativeDep::Occt);
    reify_build_utils::emit_rpath_for_bins(NativeDep::Gmsh);
    reify_build_utils::emit_rpath_for_bins(NativeDep::OpenVdb);
}
