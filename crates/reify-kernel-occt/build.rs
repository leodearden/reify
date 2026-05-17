use std::env;
use std::path::PathBuf;

use reify_build_utils::{LibLoc, NativeDep, read_soname_version};

// Pull in the shared constants so build.rs can emit the generated C++ header
// with the authoritative CPP_LINE_WIRE_MIN_LENGTH_SQ value. Using #[path] here
// avoids duplicating the literal value in build.rs — duplicating would reintroduce
// the exact drift class this task eliminates.
#[path = "src/floor_constants.rs"]
mod floor_constants;

fn main() {
    // Declare has_occt as a known cfg so rustc doesn't warn about it.
    println!("cargo::rustc-check-cfg=cfg(has_occt)");
    println!("cargo:rerun-if-env-changed=OCCT_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=OCCT_LIB_DIR");

    // Auto-detect OCCT availability.
    let LibLoc { include_dir, lib_dir } = match reify_build_utils::find(NativeDep::Occt) {
        Some(loc) => loc,
        None => {
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
    let cpp_floor_value = format!("{:.17e}", floor_constants::CPP_LINE_WIRE_MIN_LENGTH_SQ);
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

    // Pin OCCT linkage to the exact SONAME filename in `lib_dir` instead of
    // letting ld pick `libTKernel.so` from the first `-L` dir that has one.
    // Reason: `reify-cli` and `reify-gui` transitively depend on
    // `reify-kernel-gmsh` (and `-openvdb`) whose build.rs scripts add
    // `/opt/reify-deps/lib` to the link search path. The conda env ships OCCT
    // 7.9 there as a transitive of gmsh=4.15.2, so ld would otherwise resolve
    // OCCT against 7.9 and the resulting binary would NEED `libTKernel.so.7.9`
    // — which only exists under `/etc/ld.so.conf.d/reify-deps.conf`. Pinning
    // to the OCCT-detected lib_dir's exact filename (e.g. `:libTKernel.so.7.8`
    // on the system path) makes the link impervious to the gmsh-induced `-L`.
    //
    // The first-level symlink target is read at build time so this adapts to
    // whatever OCCT version is actually installed (7.8 on system today, 7.9
    // tomorrow if Debian/Ubuntu update). Fallback `"7.8"` is the current
    // system version (`libTKernel.so → libTKernel.so.7.8`) on the dev box.
    let so_version = read_soname_version(&lib_dir, "TKernel").unwrap_or_else(|| {
        println!(
            "cargo:warning=Could not detect OCCT SONAME from {} — falling back to '7.8'. \
             If your OCCT install uses a different version, set OCCT_LIB_DIR.",
            lib_dir.display()
        );
        "7.8".to_string()
    });
    for lib in &occt_libs {
        // `dylib:+verbatim` passes the literal name to the linker without the
        // usual `lib<NAME>` / `.so` rewrites — ld then accepts the exact
        // filename match. Without `+verbatim`, rustc rejects the leading `:`
        // ("library name must not be empty") and a bare `lib<NAME>.so.<VER>`
        // would be normalised to `-llib<NAME>.so.<VER>` which has no match
        // anywhere.
        println!("cargo:rustc-link-lib=dylib:+verbatim=lib{lib}.so.{so_version}");
    }

    // Set rpath so test binaries in *this* package resolve OCCT at runtime.
    // Note: `rustc-link-arg` only applies to bin/test targets in this same
    // package — workspace binaries (`reify`, `reify-gui`) get their RPATH via
    // `reify_build_utils::emit_rpath_for_bins(NativeDep::Occt)` in their own
    // build.rs.
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());

    // Also need to link the C++ standard library
    println!("cargo:rustc-link-lib=dylib=stdc++");
}
