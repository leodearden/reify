// Link against the system-installed libslvs (from libslvs1-dev package).
//
// Detection order:
//   1. pkg-config probe for "slvs"
//   2. SLVS_LIB_DIR env var (manual override)
//   3. System default paths (try linking directly)
//
// If none succeed, the build fails with a clear, actionable error message.
//
// For custom library locations, set:
//   SLVS_LIB_DIR   — directory containing libslvs.so
//   SLVS_INCLUDE_DIR — directory containing slvs.h (optional, for diagnostics)

fn main() {
    // Tell cargo to re-run if these env vars change.
    println!("cargo:rerun-if-env-changed=SLVS_LIB_DIR");
    println!("cargo:rerun-if-env-changed=SLVS_INCLUDE_DIR");

    // Allow build.rs to set custom cfg flags.
    println!("cargo::rustc-check-cfg=cfg(slvs_not_found)");

    // 1. Try pkg-config first — it auto-emits the right link flags.
    if pkg_config::Config::new()
        .atleast_version("1.0")
        .probe("slvs")
        .is_ok()
    {
        // pkg-config found it and emitted cargo:rustc-link-lib and
        // cargo:rustc-link-search directives automatically.
        return;
    }

    // 2. Fall back to SLVS_LIB_DIR env var.
    if let Ok(lib_dir) = std::env::var("SLVS_LIB_DIR") {
        println!("cargo:rustc-link-search=native={lib_dir}");
        println!("cargo:rustc-link-lib=slvs");

        // Optional: diagnostic check for the header.
        let header_found = std::env::var("SLVS_INCLUDE_DIR")
            .map(|dir| std::path::Path::new(&dir).join("slvs.h").exists())
            .unwrap_or(false);
        if !header_found {
            println!(
                "cargo:warning=slvs.h not found — \
                 consider setting SLVS_INCLUDE_DIR to the directory containing slvs.h"
            );
        }
        return;
    }

    // 3. Try linking against system default paths.
    //    libslvs1-dev on Ubuntu/Debian installs to /usr/lib/<arch>/ but
    //    doesn't ship a .pc file, so pkg-config misses it. The linker
    //    finds it via default search paths.
    //
    //    We check for the header as a proxy for the library being installed.
    let header_exists = std::path::Path::new("/usr/include/slvs.h").exists();
    if header_exists {
        println!("cargo:rustc-link-lib=slvs");
        return;
    }

    // 4. Nothing found — emit a clear error.
    println!(
        "cargo:warning=libslvs not found. Install with: sudo apt install libslvs1-dev"
    );
    println!(
        "cargo:warning=Or set SLVS_LIB_DIR to the directory containing libslvs.so"
    );
    println!("cargo:rustc-cfg=slvs_not_found");
    println!("cargo:rustc-link-lib=slvs");
}
