use std::env;
use std::path::PathBuf;

fn find_include_dir() -> Option<PathBuf> {
    if let Ok(dir) = env::var("GMSH_INCLUDE_DIR") {
        return Some(PathBuf::from(dir));
    }

    let search_include = [
        "/opt/reify-deps/include",
        "/usr/include",
        "/usr/local/include",
    ];

    for p in &search_include {
        let path = PathBuf::from(p);
        if path.join("gmshc.h").exists() {
            return Some(path);
        }
    }

    None
}

fn find_lib_dir() -> Option<PathBuf> {
    if let Ok(dir) = env::var("GMSH_LIB_DIR") {
        return Some(PathBuf::from(dir));
    }

    let search_lib = [
        "/opt/reify-deps/lib",
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib",
        "/usr/local/lib",
    ];

    for p in &search_lib {
        let path = PathBuf::from(p);
        if path.join("libgmsh.so").exists() {
            return Some(path);
        }
    }

    None
}

fn main() {
    // Declare has_gmsh as a known cfg so rustc doesn't warn about it.
    println!("cargo::rustc-check-cfg=cfg(has_gmsh)");

    // Tell cargo to re-run this build script when the Gmsh env vars change.
    // Without these directives, cargo caches the build script output and won't
    // re-run it after the user installs libgmsh and sets GMSH_INCLUDE_DIR /
    // GMSH_LIB_DIR — so `has_gmsh` would stay un-set forever in that cached
    // build. Mirrors crates/reify-kernel-openvdb/build.rs:61-62.
    println!("cargo:rerun-if-env-changed=GMSH_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=GMSH_LIB_DIR");
    // Re-run the build script when the FFI binding declarations change so
    // updated extern "C" declarations trigger a re-link (mirrors
    // crates/reify-kernel-openvdb/build.rs:65).
    println!("cargo:rerun-if-changed=src/ffi.rs");

    // Auto-detect Gmsh availability. Same fail-soft posture as
    // `crates/reify-kernel-occt/build.rs`: if the system lacks libgmsh, the
    // crate still compiles — only the stub kernel is exposed.
    let include_dir = find_include_dir();
    let lib_dir = find_lib_dir();

    let (_include_dir, lib_dir) = match (include_dir, lib_dir) {
        (Some(inc), Some(lib)) => (inc, lib),
        _ => {
            // Gmsh not found — emit a warning and exit gracefully.
            // The crate will compile with the stub kernel only.
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

    // Embed RPATH so test/release binaries resolve libgmsh at runtime
    // without requiring `/etc/ld.so.conf.d/reify-deps.conf` (which would leak
    // ALL conda-bundled runtime libs into the global linker cache and shadow
    // system libs — see scripts/setup-dev.sh:252-254 + the OCCT precedent).
    // Mirrors crates/reify-kernel-openvdb/build.rs:134 and
    // crates/reify-kernel-occt/build.rs:158.
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());
}
