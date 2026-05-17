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
    //
    // OpenVDB is not currently in `reify-cli`'s closure; if it ever is, add
    // `emit_rpath_for_bins(NativeDep::OpenVdb)` here.
    reify_build_utils::emit_rpath_for_bins(NativeDep::Occt);
    reify_build_utils::emit_rpath_for_bins(NativeDep::Gmsh);
}
