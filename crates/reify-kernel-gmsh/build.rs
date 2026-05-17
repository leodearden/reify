use reify_build_utils::{LibLoc, NativeDep};

fn main() {
    // Declare has_gmsh as a known cfg so rustc doesn't warn about it.
    println!("cargo::rustc-check-cfg=cfg(has_gmsh)");

    // Tell cargo to re-run this build script when the Gmsh env vars change.
    // Without these directives, cargo caches the build script output and won't
    // re-run it after the user installs libgmsh and sets GMSH_INCLUDE_DIR /
    // GMSH_LIB_DIR — so `has_gmsh` would stay un-set forever in that cached
    // build. Mirrors crates/reify-kernel-openvdb/build.rs.
    println!("cargo:rerun-if-env-changed=GMSH_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=GMSH_LIB_DIR");
    // Re-run the build script when the FFI binding declarations change so
    // updated extern "C" declarations trigger a re-link.
    println!("cargo:rerun-if-changed=src/ffi.rs");

    // Auto-detect Gmsh availability. Same fail-soft posture as
    // `crates/reify-kernel-occt/build.rs`: if the system lacks libgmsh, the
    // crate still compiles — only the stub kernel is exposed.
    let LibLoc { include_dir: _include_dir, lib_dir } =
        match reify_build_utils::find(NativeDep::Gmsh) {
            Some(loc) => loc,
            None => {
                println!(
                    "cargo:warning=Gmsh libraries not found. \
                     Building without Gmsh support (stub kernel only). \
                     Set GMSH_INCLUDE_DIR / GMSH_LIB_DIR or install gmsh."
                );
                return;
            }
        };

    // Gmsh found — enable the has_gmsh cfg flag.
    println!("cargo:rustc-cfg=has_gmsh");

    // Link against libgmsh. The native search dir is the lib directory
    // detected above; the dylib name is "gmsh" (libgmsh.so).
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=dylib=gmsh");

    // Embed RPATH so in-package test/release binaries resolve libgmsh at
    // runtime without requiring `/etc/ld.so.conf.d/reify-deps.conf`.
    // Workspace binaries (`reify`, `reify-gui`) get RPATH via
    // `reify_build_utils::emit_rpath_for_bins(NativeDep::Gmsh)` in their own
    // build.rs — `rustc-link-arg` does not propagate across package
    // boundaries.
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());

    // Expose the resolved lib_dir to in-package tests via env! at compile
    // time. `tests/rpath_smoke.rs` asserts the embedded RPATH/RUNPATH entry
    // references *this* exact lib_dir, which honours `GMSH_LIB_DIR`
    // overrides and the non-canonical fallback paths.
    println!("cargo:rustc-env=REIFY_GMSH_LIB_DIR={}", lib_dir.display());
}
