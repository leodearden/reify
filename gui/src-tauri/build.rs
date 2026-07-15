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
    #[cfg(feature = "gui")]
    {
        use reify_build_utils::NativeDep;
        // Mechanism A″ (task #5192): give the `reify-gui` binary a direct
        // NEEDED libtbb.so.12 via the tbb-only pin dir, prepended FIRST in
        // RUNPATH — BEFORE the native-dep rpath emissions below, so
        // tbb-pin lands first in the binary's DT_RUNPATH ahead of
        // /opt/reify-deps/lib et al.
        //
        // `_for_bins` only — no `emit_tbb_pin_for_tests()` — intentionally
        // mirroring the existing emit_rpath_for_bins-only posture below
        // (gui never calls emit_rpath_for_tests either). gui test binaries
        // only spawn the built `reify-gui` executable out-of-process; none
        // load OCCT/tbb in-process. If an in-process OCCT-loading gui test
        // is ever introduced, it will hit the same system-libtbb
        // undefined-symbol crash this task fixes and will need
        // emit_tbb_pin_for_tests() added here.
        reify_build_utils::emit_tbb_pin_for_bins();
        reify_build_utils::emit_rpath_for_bins(NativeDep::Occt);
        reify_build_utils::emit_rpath_for_bins(NativeDep::Gmsh);
    }

    #[cfg(feature = "gui")]
    tauri_build::build();
}
