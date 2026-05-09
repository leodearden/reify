//! Pin: compiled `reify-kernel-gmsh` test binaries embed an RPATH /
//! RUNPATH entry for `/opt/reify-deps/lib`.
//!
//! Done-criterion addendum: the build script in `pre-2` emits
//! `cargo:rustc-link-arg=-Wl,-rpath,<lib_dir>` so test/release binaries
//! resolve `libgmsh.so` at runtime without depending on
//! `/etc/ld.so.conf.d/reify-deps.conf` (which would shadow system libs —
//! see `scripts/setup-dev.sh:252-254`).
//!
//! The test reads `readelf -d` on the running binary's own ELF and
//! asserts the output contains `/opt/reify-deps/lib`. A regression that
//! drops the link-arg (e.g. someone restoring the comment block in
//! `build.rs`) would surface here.
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
/// references `/opt/reify-deps/lib`. If `readelf` is not on PATH (e.g.
/// minimal CI image), skip with a stderr note rather than failing —
/// mirrors the OCCT-availability skip pattern.
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
    let mentions_reify_deps = stdout.contains("/opt/reify-deps/lib");

    assert!(
        has_rpath,
        "readelf -d {} produced no (RPATH) or (RUNPATH) entry — \
         build.rs may have regressed and dropped the rustc-link-arg \
         -Wl,-rpath,<lib_dir> directive.\n\nFull readelf -d output:\n{stdout}",
        exe.display(),
    );
    assert!(
        mentions_reify_deps,
        "readelf -d {} has an RPATH/RUNPATH entry but it does not reference \
         /opt/reify-deps/lib — the embedded path may have drifted off the \
         conda-forge install root.\n\nFull readelf -d output:\n{stdout}",
        exe.display(),
    );
}
