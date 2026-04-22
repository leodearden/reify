use std::env;
use std::path::PathBuf;

// Pull in the shared constants so build.rs can emit the generated C++ header
// with the authoritative CPP_LINE_WIRE_MIN_LENGTH_SQ value. Using #[path] here
// avoids duplicating the literal value in build.rs — duplicating would reintroduce
// the exact drift class this task eliminates.
#[path = "src/floor_constants.rs"]
mod floor_constants;

fn find_include_dir() -> Option<PathBuf> {
    if let Ok(dir) = env::var("OCCT_INCLUDE_DIR") {
        return Some(PathBuf::from(dir));
    }

    let search_include = [
        "/usr/include/opencascade",
        "/usr/local/include/opencascade",
        "/snap/freecad/current/usr/include/opencascade",
    ];

    for p in &search_include {
        let path = PathBuf::from(p);
        if path.join("Standard_Failure.hxx").exists() {
            return Some(path);
        }
    }

    // Also try numbered snap directories
    if let Ok(entries) = std::fs::read_dir("/snap/freecad") {
        for entry in entries.flatten() {
            let candidate = entry.path().join("usr/include/opencascade");
            if candidate.join("Standard_Failure.hxx").exists() {
                return Some(candidate);
            }
        }
    }

    None
}

fn find_lib_dir() -> Option<PathBuf> {
    if let Ok(dir) = env::var("OCCT_LIB_DIR") {
        return Some(PathBuf::from(dir));
    }

    let search_lib = [
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib",
        "/usr/local/lib",
        "/snap/freecad/current/usr/lib",
    ];

    for p in &search_lib {
        let path = PathBuf::from(p);
        if path.join("libTKernel.so").exists() {
            return Some(path);
        }
    }

    if let Ok(entries) = std::fs::read_dir("/snap/freecad") {
        for entry in entries.flatten() {
            let candidate = entry.path().join("usr/lib");
            if candidate.join("libTKernel.so").exists() {
                return Some(candidate);
            }
        }
    }

    None
}

fn main() {
    // Declare has_occt as a known cfg so rustc doesn't warn about it.
    println!("cargo::rustc-check-cfg=cfg(has_occt)");

    // Auto-detect OCCT availability.
    let include_dir = find_include_dir();
    let lib_dir = find_lib_dir();

    let (include_dir, lib_dir) = match (include_dir, lib_dir) {
        (Some(inc), Some(lib)) => (inc, lib),
        _ => {
            // OCCT not found — emit a warning and exit gracefully.
            // The crate will compile with stub types instead of FFI bindings.
            println!(
                "cargo:warning=OCCT libraries not found. \
                 Building without OCCT support (stub types only). \
                 Set OCCT_INCLUDE_DIR / OCCT_LIB_DIR or install libocct-*-dev."
            );
            return;
        }
    };

    // OCCT found — enable the has_occt cfg flag.
    println!("cargo:rustc-cfg=has_occt");

    // Emit the generated C++ header so occt_wrapper.cpp can include it.
    println!("cargo:rerun-if-changed=src/floor_constants.rs");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let cpp_floor_value = format!("{:e}", floor_constants::CPP_LINE_WIRE_MIN_LENGTH_SQ);
    let header_content = format!(
        "#pragma once\n\
         // Auto-generated from src/floor_constants.rs by build.rs. Do not edit.\n\
         namespace occt {{\n\
         constexpr double CPP_LINE_WIRE_MIN_LENGTH_SQ = {cpp_floor_value};\n\
         }}\n"
    );
    std::fs::write(out_dir.join("line_wire_floors.h"), header_content)
        .expect("failed to write line_wire_floors.h");

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
        .include(&out_dir)
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
        "TKG2d",
        "TKG3d",
        "TKGeomAlgo",
        "TKGeomBase",
        "TKShHealing",
        "TKBool",
        "TKOffset",
    ];
    for lib in &occt_libs {
        println!("cargo:rustc-link-lib=dylib={}", lib);
    }

    // Set rpath so the dynamic linker can find OCCT libs at runtime
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());

    // Also need to link the C++ standard library
    println!("cargo:rustc-link-lib=dylib=stdc++");
}
