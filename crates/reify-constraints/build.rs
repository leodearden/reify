// Link against the system-installed libslvs (from libslvs1-dev package).
//
// If the build fails with linker errors about missing -lslvs:
//   sudo apt install libslvs1-dev
//
// For custom library locations, set:
//   SLVS_LIB_DIR — directory containing libslvs.so
//   SLVS_INCLUDE_DIR — directory containing slvs.h

fn main() {
    // Support custom library locations via environment variables.
    if let Ok(lib_dir) = std::env::var("SLVS_LIB_DIR") {
        println!("cargo:rustc-link-search=native={lib_dir}");
    }

    println!("cargo:rustc-link-lib=slvs");

    // Diagnostic: check for the header to give a helpful message if the
    // library is not installed.
    let header_found = std::env::var("SLVS_INCLUDE_DIR")
        .map(|dir| std::path::Path::new(&dir).join("slvs.h").exists())
        .unwrap_or_else(|_| std::path::Path::new("/usr/include/slvs.h").exists());

    if !header_found {
        println!(
            "cargo:warning=slvs.h not found — \
             install libslvs1-dev (sudo apt install libslvs1-dev) \
             or set SLVS_LIB_DIR and SLVS_INCLUDE_DIR"
        );
    }
}
