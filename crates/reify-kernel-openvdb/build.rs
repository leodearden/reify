use std::env;
use std::path::PathBuf;

fn find_include_dir() -> Option<PathBuf> {
    // 1. Environment override (highest priority).
    if let Ok(dir) = env::var("OPENVDB_INCLUDE_DIR") {
        return Some(PathBuf::from(dir));
    }

    // 2. Canonical conda-forge env installed by setup-dev.sh.
    // 3. /usr/local (from-source builds).
    // 4. /usr (system package installs, e.g. libopenvdb-dev on Ubuntu).
    let candidates = [
        "/opt/reify-deps/include",
        "/usr/local/include",
        "/usr/include",
    ];

    for p in &candidates {
        let path = PathBuf::from(p);
        if path.join("openvdb/openvdb.h").exists() {
            return Some(path);
        }
    }

    None
}

fn find_lib_dir() -> Option<PathBuf> {
    // 1. Environment override (highest priority).
    if let Ok(dir) = env::var("OPENVDB_LIB_DIR") {
        return Some(PathBuf::from(dir));
    }

    // 2. Canonical conda-forge env installed by setup-dev.sh.
    // 3. /usr/local (from-source builds).
    // 4. /usr/lib/x86_64-linux-gnu (Ubuntu apt installs).
    // 5. /usr/lib (generic fallback).
    let candidates = [
        "/opt/reify-deps/lib",
        "/usr/local/lib",
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib",
    ];

    for p in &candidates {
        let path = PathBuf::from(p);
        if path.join("libopenvdb.so").exists() {
            return Some(path);
        }
    }

    None
}

fn main() {
    // Declare has_openvdb as a known cfg so rustc doesn't warn about it.
    println!("cargo::rustc-check-cfg=cfg(has_openvdb)");

    // Rerun if the detection inputs change.
    println!("cargo:rerun-if-env-changed=OPENVDB_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=OPENVDB_LIB_DIR");
    println!("cargo:rerun-if-changed=cpp/openvdb_wrapper.h");
    println!("cargo:rerun-if-changed=cpp/openvdb_wrapper.cpp");
    println!("cargo:rerun-if-changed=src/ffi.rs");

    let include_dir = find_include_dir();
    let lib_dir = find_lib_dir();

    let (include_dir, lib_dir) = match (include_dir, lib_dir) {
        (Some(inc), Some(lib)) => (inc, lib),
        _ => {
            // OpenVDB not found — emit a warning and exit gracefully.
            // The crate compiles in stub-only mode without has_openvdb.
            println!(
                "cargo:warning=OpenVDB libraries not found. \
                 Building without OpenVDB support (stub kernel only). \
                 Set OPENVDB_INCLUDE_DIR / OPENVDB_LIB_DIR or install \
                 libopenvdb-dev (or run setup-dev.sh to install the \
                 conda-forge env at /opt/reify-deps)."
            );
            return;
        }
    };

    // OpenVDB found — enable the has_openvdb cfg flag.
    println!("cargo:rustc-cfg=has_openvdb");

    // Build the cxx bridge + C++ wrapper.
    let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let mut build = cxx_build::bridge("src/ffi.rs");
    build
        .file("cpp/openvdb_wrapper.cpp")
        .include(&include_dir)
        .include(crate_dir.join("cpp"))
        .std("c++17")
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-deprecated-declarations")
        // Suppress OpenVDB's internal deprecation warnings (common in 13.x).
        .flag_if_supported("-Wno-deprecated");

    build.compile("reify_openvdb_wrapper");

    // Link OpenVDB and its required transitive dependencies.
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=dylib=openvdb");
    println!("cargo:rustc-link-lib=dylib=tbb");
    println!("cargo:rustc-link-lib=dylib=stdc++");

    // Embed RPATH so binaries resolve libopenvdb at runtime without
    // requiring /etc/ld.so.conf.d/reify-deps.conf (which leaks all conda
    // runtime libs into the system linker cache — a known cmake conflict).
    // Mirrors crates/reify-kernel-occt/build.rs:158.
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());
}
