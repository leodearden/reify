fn main() {
    // Embed RUNPATH for native deps transitively linked by `reify-gui`.
    // `rustc-link-arg-bins` is required because Cargo does not propagate
    // `rustc-link-arg` directives across package boundaries — kernel
    // adapter build.rs scripts emit RPATH only for their own in-package
    // test binaries, not for workspace binaries like `reify-gui`. See
    // `crates/reify-cli/build.rs` for the same pattern.
    //
    // `reify-gui` transitively pulls:
    //   - OCCT via `reify-kernel-occt` (direct optional dep behind `gui`
    //     feature)
    //   - Gmsh via `reify-eval → reify-solver-elastic → reify-kernel-gmsh`
    //   - OpenVDB via `reify-eval → reify-kernel-openvdb` (since task 3576)

    // A build-script cfg does not propagate to dependents, so this crate
    // detects OpenVDB itself, as `crates/reify-cli/build.rs` does. It is
    // feature-independent: the default-feature lib tests gate on it too.
    println!("cargo::rustc-check-cfg=cfg(has_openvdb)");
    if reify_build_utils::find(reify_build_utils::NativeDep::OpenVdb).is_some() {
        println!("cargo:rustc-cfg=has_openvdb");
    }

    #[cfg(feature = "gui")]
    {
        use reify_build_utils::NativeDep;
        // Mechanism A″ (task #5192): give the `reify-gui` binary a direct
        // NEEDED libtbb.so.12 via the tbb-only pin dir, prepended FIRST in
        // RUNPATH — BEFORE the native-dep rpath emissions below, so
        // tbb-pin lands first in the binary's DT_RUNPATH ahead of
        // /opt/reify-deps/lib et al.
        //
        // `_for_bins` only — no `emit_tbb_pin_for_tests()` — mirroring the
        // emit_rpath_for_bins-only posture below. If an in-process
        // OCCT/OpenVDB-loading gui test ever hits the system-libtbb
        // undefined-symbol crash #5192 fixed for the binary, add
        // emit_tbb_pin_for_tests() here.
        reify_build_utils::emit_tbb_pin_for_bins();
        reify_build_utils::emit_rpath_for_bins(NativeDep::Occt);
        reify_build_utils::emit_rpath_for_bins(NativeDep::Gmsh);
        reify_build_utils::emit_rpath_for_bins(NativeDep::OpenVdb);
    }

    #[cfg(feature = "gui")]
    tauri_build::build();
}
