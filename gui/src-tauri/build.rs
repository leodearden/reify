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
        reify_build_utils::emit_rpath_for_bins(NativeDep::Occt);
        reify_build_utils::emit_rpath_for_bins(NativeDep::Gmsh);
    }

    #[cfg(feature = "gui")]
    tauri_build::build();
}
