use std::env;
use std::path::PathBuf;

fn main() {
    // Determine OCCT paths from environment or well-known locations.
    let occt_include = env::var("OCCT_INCLUDE_DIR").ok().map(PathBuf::from);
    let occt_lib = env::var("OCCT_LIB_DIR").ok().map(PathBuf::from);

    // Well-known search paths (system, snap FreeCAD)
    let search_include = [
        "/usr/include/opencascade",
        "/usr/local/include/opencascade",
        "/snap/freecad/current/usr/include/opencascade",
    ];
    let search_lib = [
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib",
        "/usr/local/lib",
        "/snap/freecad/current/usr/lib",
    ];

    let include_dir = occt_include.unwrap_or_else(|| {
        for p in &search_include {
            let path = PathBuf::from(p);
            if path.join("Standard_Failure.hxx").exists() {
                return path;
            }
        }
        // Also try numbered snap directories
        if let Ok(entries) = std::fs::read_dir("/snap/freecad") {
            for entry in entries.flatten() {
                let candidate = entry.path().join("usr/include/opencascade");
                if candidate.join("Standard_Failure.hxx").exists() {
                    return candidate;
                }
            }
        }
        panic!(
            "Cannot find OCCT include directory. Set OCCT_INCLUDE_DIR or install libocct-*-dev"
        );
    });

    let lib_dir = occt_lib.unwrap_or_else(|| {
        for p in &search_lib {
            let path = PathBuf::from(p);
            if path.join("libTKernel.so").exists() {
                return path;
            }
        }
        if let Ok(entries) = std::fs::read_dir("/snap/freecad") {
            for entry in entries.flatten() {
                let candidate = entry.path().join("usr/lib");
                if candidate.join("libTKernel.so").exists() {
                    return candidate;
                }
            }
        }
        panic!("Cannot find OCCT lib directory. Set OCCT_LIB_DIR or install libocct-*-dev");
    });

    println!("cargo:rerun-if-changed=cpp/occt_wrapper.h");
    println!("cargo:rerun-if-changed=cpp/occt_wrapper.cpp");
    println!("cargo:rerun-if-changed=src/ffi.rs");

    // Build the cxx bridge + C++ wrapper
    let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let mut build = cxx_build::bridge("src/ffi.rs");
    build
        .file("cpp/occt_wrapper.cpp")
        .include(&include_dir)
        .include(crate_dir.join("cpp"))
        .std("c++17")
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-deprecated-declarations");

    build.compile("reify_occt_wrapper");

    // Link OCCT libraries
    println!("cargo:rustc-link-search=native={}", lib_dir.display());

    let occt_libs = [
        "TKernel",
        "TKMath",
        "TKBRep",
        "TKPrim",
        "TKBO",
        "TKFillet",
        "TKTopAlgo",
        "TKDESTEP",
        "TKDE",
        "TKXSBase",
        "TKMesh",
        "TKG3d",
        "TKGeomBase",
        "TKShHealing",
        "TKBool",
    ];
    for lib in &occt_libs {
        println!("cargo:rustc-link-lib=dylib={}", lib);
    }

    // Set rpath so the dynamic linker can find OCCT libs at runtime
    println!(
        "cargo:rustc-link-arg=-Wl,-rpath,{}",
        lib_dir.display()
    );

    // Also need to link the C++ standard library
    println!("cargo:rustc-link-lib=dylib=stdc++");
}
