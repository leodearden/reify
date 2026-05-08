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

    // Auto-detect Gmsh availability. Same fail-soft posture as
    // `crates/reify-kernel-occt/build.rs`: if the system lacks libgmsh, the
    // crate still compiles — only the stub kernel is exposed.
    let include_dir = find_include_dir();
    let lib_dir = find_lib_dir();

    let (_include_dir, _lib_dir) = match (include_dir, lib_dir) {
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

    // Gmsh found — enable the has_gmsh cfg flag. The actual extern "C" call
    // sequence (gmshInitialize, gmshModelMeshGenerate, …) is scaffolded as
    // follow-up work; this prerequisite just lays the detection rails so the
    // FFI lands as a contained patch later (see task 2925 analysis).
    println!("cargo:rustc-cfg=has_gmsh");

    // Note: we deliberately do NOT emit `rustc-link-search` / `rustc-link-lib`
    // here yet, because no Rust code currently links against libgmsh — the
    // stub kernel does not call into it. Linking will be added in the
    // follow-up FFI task alongside the extern "C" bindings.
}
