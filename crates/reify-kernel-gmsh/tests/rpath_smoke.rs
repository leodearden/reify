//! Pin: compiled `reify-kernel-gmsh` test binaries embed an RPATH /
//! RUNPATH entry for the libgmsh lib_dir resolved by `build.rs`.
//!
//! Done-criterion addendum: the build script in `pre-2` emits
//! `cargo:rustc-link-arg=-Wl,-rpath,<lib_dir>` so test/release binaries
//! resolve `libgmsh.so` at runtime without depending on
//! `/etc/ld.so.conf.d/reify-deps.conf` (which would shadow system libs —
//! see `scripts/setup-dev.sh:252-254`).
//!
//! The test reads `readelf -d` on the running binary's own ELF and
//! asserts the output contains the lib_dir `build.rs` actually used,
//! exposed via `cargo:rustc-env=REIFY_GMSH_LIB_DIR=<lib_dir>` (read here
//! through `env!`). The directive that `build.rs` emits to rustc is the
//! same string, so any drift between detection and emission would
//! surface here — without hardcoding `/opt/reify-deps/lib` (which
//! would falsely fail on hosts where `GMSH_LIB_DIR` overrides the path
//! or libgmsh ships under `/usr/lib/...` etc.).
//!
//! Only compiled when both `cfg(has_gmsh)` (libgmsh detected at build
//! time → RPATH directive issued) AND `target_os = "linux"` (readelf is
//! a binutils tool; macOS uses `otool -l`, Windows has no equivalent —
//! we don't claim cross-platform RPATH coverage in v0.3).

#![cfg(all(has_gmsh, target_os = "linux"))]

use std::process::Command;

/// Pin the RPATH/RUNPATH directive emitted by `build.rs` (step pre-2).
///
/// Invokes `readelf -d` on the test binary itself; asserts the output
/// references the lib_dir `build.rs` actually used (read via `env!` from
/// the `REIFY_GMSH_LIB_DIR` `cargo:rustc-env=` directive). If `readelf`
/// is not on PATH (e.g. minimal CI image), skip with a stderr note
/// rather than failing — mirrors the OCCT-availability skip pattern.
#[test]
fn compiled_test_binary_embeds_rpath_to_reify_deps_lib() {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("std::env::current_exe failed ({e}); skipping rpath check");
            return;
        }
    };

    let output = match Command::new("readelf").args(["-d"]).arg(&exe).output() {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("readelf unavailable on PATH; skipping rpath check");
            return;
        }
        Err(e) => panic!("readelf invocation failed: {e}"),
    };

    if !output.status.success() {
        eprintln!(
            "readelf -d exited non-zero (status={:?}); skipping rpath check",
            output.status,
        );
        return;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let has_rpath = stdout.contains("(RPATH)") || stdout.contains("(RUNPATH)");
    // Emitted by `build.rs` via `cargo:rustc-env=REIFY_GMSH_LIB_DIR=<lib_dir>`,
    // where `<lib_dir>` is the same path passed to
    // `cargo:rustc-link-arg=-Wl,-rpath,<lib_dir>`. Reading both from the
    // same source pins the actual emitted directive against the recorded
    // detection result, regardless of the host's install layout.
    let expected_lib_dir = env!("REIFY_GMSH_LIB_DIR");
    let mentions_lib_dir = stdout.contains(expected_lib_dir);

    assert!(
        has_rpath,
        "readelf -d {} produced no (RPATH) or (RUNPATH) entry — \
         build.rs may have regressed and dropped the rustc-link-arg \
         -Wl,-rpath,<lib_dir> directive.\n\nFull readelf -d output:\n{stdout}",
        exe.display(),
    );
    assert!(
        mentions_lib_dir,
        "readelf -d {} has an RPATH/RUNPATH entry but it does not reference \
         the lib_dir build.rs detected ({expected_lib_dir}) — detection \
         and emission may have diverged.\n\nFull readelf -d output:\n{stdout}",
        exe.display(),
    );
}
