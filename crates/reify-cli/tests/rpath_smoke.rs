//! Pin: the `reify` workspace binary embeds a RUNPATH entry and does NOT
//! link conda-forge OCCT 7.9 (which would tie it to the
//! `/etc/ld.so.conf.d/reify-deps.conf` system linker cache entry).
//!
//! This test is the binary-side complement to
//! `crates/reify-kernel-gmsh/tests/rpath_smoke.rs`, which only inspects an
//! in-package test binary. Cargo's `rustc-link-arg` directive does NOT
//! propagate across package boundaries, so the kernel adapters' RPATH
//! directives never reach this `reify` binary — `crates/reify-cli/build.rs`
//! calls `reify_build_utils::emit_rpath_for_bins` to fix that, and this
//! test pins the fix.

#![cfg(target_os = "linux")]

use std::process::Command;

/// `readelf -d <reify>` must contain an (RPATH) or (RUNPATH) entry.
/// Regression guard for Bug 1 in `~/.claude/plans/ldconfig-removal.md`:
/// kernel adapter `rustc-link-arg` directives don't cross package
/// boundaries; the workspace binary needs its own build.rs emitter.
#[test]
fn reify_binary_embeds_runpath() {
    let exe = env!("CARGO_BIN_EXE_reify");
    let Some(stdout) = readelf_d(exe) else {
        return;
    };
    let has_rpath = stdout.contains("(RPATH)") || stdout.contains("(RUNPATH)");
    assert!(
        has_rpath,
        "readelf -d {exe} produced no (RPATH) or (RUNPATH) entry — \
         crates/reify-cli/build.rs may have regressed and dropped its \
         emit_rpath_for_bins call, or reify_build_utils failed to detect \
         the native lib_dirs on this host.\n\nFull readelf -d output:\n{stdout}"
    );
}

/// The `reify` binary's NEEDED entries for OCCT libs must reference the
/// system SONAME (e.g. `libTKernel.so.7.8`), never `.7.9`. Regression guard
/// for Bug 2: the conda env at `/opt/reify-deps/lib` ships OCCT 7.9 as a
/// transitive dep of gmsh=4.15.2; without SONAME pinning in
/// `crates/reify-kernel-occt/build.rs`, the conda OCCT 7.9 silently shadows
/// the system OCCT 7.8 in the link.
///
/// The test is skipped when no OCCT NEEDED entries are present (e.g. a
/// stub-only build with OCCT undetected at build time).
#[test]
fn reify_binary_does_not_link_conda_occt_7_9() {
    let exe = env!("CARGO_BIN_EXE_reify");
    let Some(stdout) = readelf_d(exe) else {
        return;
    };
    let needed_tk: Vec<&str> = stdout
        .lines()
        .filter(|l| l.contains("(NEEDED)") && l.contains("libTK"))
        .collect();
    if needed_tk.is_empty() {
        eprintln!("no NEEDED libTK* entries in {exe}; OCCT likely stubbed — skipping");
        return;
    }
    let leaks_7_9: Vec<&&str> = needed_tk.iter().filter(|l| l.contains(".7.9")).collect();
    assert!(
        leaks_7_9.is_empty(),
        "{exe} NEEDS conda-forge OCCT 7.9 libs (set by SONAME pinning bug \
         in reify-kernel-occt/build.rs):\n{leaks}\n\nFull NEEDED libTK* \
         lines:\n{all}",
        leaks = leaks_7_9
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
        all = needed_tk.join("\n"),
    );
}

/// Run `readelf -d <path>` and return its stdout. Returns `None` (skip)
/// when readelf is unavailable or fails — matching the existing skip
/// pattern in `crates/reify-kernel-gmsh/tests/rpath_smoke.rs`.
fn readelf_d(path: &str) -> Option<String> {
    let output = match Command::new("readelf").args(["-d", path]).output() {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("readelf unavailable on PATH; skipping");
            return None;
        }
        Err(e) => panic!("readelf invocation failed: {e}"),
    };
    if !output.status.success() {
        eprintln!(
            "readelf -d exited non-zero (status={:?}); skipping",
            output.status
        );
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}
