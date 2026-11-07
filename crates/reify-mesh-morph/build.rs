//! Build script for `reify-mesh-morph`.
//!
//! Exists solely so the crate's own test binaries can gate on gmsh
//! availability and resolve `libgmsh` at runtime. Both halves are required
//! and NEITHER is redundant with the gmsh crate's own build script — cargo
//! build-script directives do not cross package boundaries:
//!
//! - `has_gmsh` detection is copied from `crates/reify-eval/build.rs:73-86`,
//!   which carries the doctrine verbatim: "A cfg emitted by the gmsh crate's
//!   build.rs does NOT propagate to dependents — each crate that wants to
//!   gate on gmsh availability must detect it itself." Without this block
//!   every `#[cfg(has_gmsh)]` in this crate is permanently false and the
//!   gmsh arm of `tests/morph_scale_characterisation.rs` silently compiles
//!   to nothing.
//! - `emit_rpath_for_tests` is copied from `crates/reify-solver-elastic/build.rs`,
//!   which exists for exactly this reason: `rustc-link-arg=-Wl,-rpath,…`
//!   likewise does not propagate, so without it a test binary that links
//!   libgmsh launches with empty RUNPATH and dies with
//!   `libgmsh.so.4.15: cannot open shared object file`.
//!
//! Do not "simplify" either half away.

use reify_build_utils::NativeDep;

fn main() {
    // Declare has_gmsh as a known cfg so rustc doesn't warn about unknown cfgs.
    println!("cargo::rustc-check-cfg=cfg(has_gmsh)");
    if reify_build_utils::find(NativeDep::Gmsh).is_some() {
        println!("cargo:rustc-cfg=has_gmsh");
    }
    // Emit RPATH so test binaries that link libgmsh resolve it at runtime.
    reify_build_utils::emit_rpath_for_tests(NativeDep::Gmsh);

    // Re-run this build script whenever it changes itself.
    println!("cargo:rerun-if-changed=build.rs");
}
