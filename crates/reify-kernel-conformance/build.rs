//! Build script for `reify-kernel-conformance` — declares + conditionally
//! sets `has_occt` and `has_gmsh` for this crate's own test binaries.
//!
//! A `cfg` emitted by a dependency crate's build script (e.g.
//! `reify-kernel-occt/build.rs`'s `has_occt`) does NOT propagate to
//! dependent crates — each crate that wants to gate its own code on native
//! availability must detect it itself. This mirrors
//! `crates/reify-eval/build.rs:60-83` (has_gmsh) and
//! `crates/reify-kernel-occt/build.rs:15-35` (has_occt).
//!
//! Also emits RPATH for both native deps so this crate's test binaries
//! (which transitively link `libTKernel*`/`libgmsh*` via the `reify-kernel-occt`
//! / `reify-kernel-gmsh` dev-deps) resolve the shared libraries at runtime —
//! `rustc-link-arg` directives from THOSE crates' own build scripts apply
//! only to bin/test targets in their own package, not here.

fn main() {
    // Declare + conditionally set has_occt.
    println!("cargo::rustc-check-cfg=cfg(has_occt)");
    if reify_build_utils::find(reify_build_utils::NativeDep::Occt).is_some() {
        println!("cargo:rustc-cfg=has_occt");
    }
    reify_build_utils::emit_rpath_for_tests(reify_build_utils::NativeDep::Occt);

    // Declare + conditionally set has_gmsh.
    println!("cargo::rustc-check-cfg=cfg(has_gmsh)");
    if reify_build_utils::find(reify_build_utils::NativeDep::Gmsh).is_some() {
        println!("cargo:rustc-cfg=has_gmsh");
    }
    reify_build_utils::emit_rpath_for_tests(reify_build_utils::NativeDep::Gmsh);

    println!("cargo:rerun-if-changed=build.rs");
}
