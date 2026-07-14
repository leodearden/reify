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

// ---------------------------------------------------------------------
// Mechanism A'' (task #5192): tbb-only RUNPATH pin dir + forced direct
// `NEEDED libtbb.so.12`, proven at RUNTIME (not just readelf token order —
// DT_RUNPATH's non-transitivity makes a token-order-only check phantom
// green: it can look right while the transitive libTKernel->libtbb edge
// still binds the system lib at load time). See CLAUDE.md "Native deps".
// ---------------------------------------------------------------------

const TBB_PIN_DIR: &str = "/opt/reify-deps/tbb-pin";

/// Whether this host has the tbb-pin dir materialized
/// (`scripts/build-manifold-deps.sh`). Every runtime assertion below skips
/// when it is absent, so CI images without the deps host degrade to skip
/// instead of false-failing.
fn tbb_pin_present() -> bool {
    std::path::Path::new(TBB_PIN_DIR).join("libtbb.so.12").exists()
}

/// Run `env -u LD_LIBRARY_PATH ldd <path>` and return its stdout. `None`
/// (skip) when `env`/`ldd` is unavailable or the invocation itself fails —
/// mirrors `readelf_d`'s skip posture.
fn bare_ldd(path: &str) -> Option<String> {
    let output = match Command::new("env").args(["-u", "LD_LIBRARY_PATH", "ldd", path]).output() {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("env/ldd unavailable on PATH; skipping");
            return None;
        }
        Err(e) => panic!("ldd invocation failed: {e}"),
    };
    if !output.status.success() {
        eprintln!("ldd exited non-zero (status={:?}); skipping", output.status);
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Extract the resolved path from an `ldd` output line of the form
/// `<soname> => <path> (0x...)`. `None` if `soname` has no such line (e.g.
/// not NEEDED by this binary at all).
fn ldd_resolved_path<'a>(ldd_output: &'a str, soname: &str) -> Option<&'a str> {
    let needle = format!("{soname} =>");
    ldd_output.lines().find_map(|line| {
        line.trim().strip_prefix(needle.as_str())?.split_whitespace().next()
    })
}

/// The repo root, derived from this crate's manifest dir
/// (`crates/reify-cli/../..`).
fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should canonicalize")
}

/// (a) RUNTIME RESOLUTION (primary). `env -u LD_LIBRARY_PATH ldd <reify>`
/// must resolve `libtbb.so.12` to the deps copy — canonicalized, so it
/// tolerates the tbb-pin symlink hop (`tbb-pin/libtbb.so.12` ->
/// `lib/libtbb.so.12` -> `lib/libtbb.so.12.18`).
#[test]
fn reify_binary_ldd_resolves_libtbb_to_deps_lib() {
    let exe = env!("CARGO_BIN_EXE_reify");
    if !tbb_pin_present() {
        eprintln!("{TBB_PIN_DIR}/libtbb.so.12 absent on this host; skipping");
        return;
    }
    let Some(ldd_out) = bare_ldd(exe) else {
        return;
    };
    let Some(resolved) = ldd_resolved_path(&ldd_out, "libtbb.so.12") else {
        eprintln!("no libtbb.so.12 line in ldd output for {exe}; OCCT likely stubbed — skipping");
        return;
    };
    let canon = std::fs::canonicalize(resolved)
        .unwrap_or_else(|e| panic!("canonicalize {resolved} (resolved by ldd for {exe}): {e}"));
    let expected = std::fs::canonicalize("/opt/reify-deps/lib/libtbb.so.12.18")
        .expect("deps libtbb.so.12.18 must exist alongside a materialized tbb-pin dir");
    assert_eq!(
        canon, expected,
        "bare `env -u LD_LIBRARY_PATH ldd {exe}` resolves libtbb.so.12 to {resolved} \
         (canonicalized: {canon:?}), not the deps copy ({expected:?}) — either the \
         binary lacks a direct NEEDED libtbb.so.12, or {TBB_PIN_DIR} is not first in \
         its RUNPATH (crates/reify-cli/build.rs may be missing the \
         emit_tbb_pin_for_bins() call)."
    );
}

/// (b) BARE-LAUNCH SMOKE (primary). A bare `env -u LD_LIBRARY_PATH <reify>
/// eval examples/bracket.ri` must not die with a dynamic-linker undefined
/// symbol crash (the literal failure mode this task fixes:
/// `get_thread_reference_vertex` missing from the system libtbb.so.12).
#[test]
fn reify_binary_bare_launch_does_not_crash_on_occt_example() {
    let exe = env!("CARGO_BIN_EXE_reify");
    if !tbb_pin_present() {
        eprintln!("{TBB_PIN_DIR}/libtbb.so.12 absent on this host; skipping bare-launch smoke");
        return;
    }
    let example = repo_root().join("examples/bracket.ri");
    if !example.exists() {
        eprintln!("example {example:?} missing; skipping bare-launch smoke");
        return;
    }
    let output = Command::new("env")
        .arg("-u")
        .arg("LD_LIBRARY_PATH")
        .arg(exe)
        .arg("eval")
        .arg(&example)
        .output()
        .expect("failed to spawn bare `reify eval`");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("undefined symbol"),
        "bare launch of {exe} eval {example:?} crashed with a dynamic-linker \
         undefined symbol error:\n{stderr}"
    );
    assert!(
        output.status.success(),
        "bare launch of {exe} eval {example:?} exited non-zero (status={:?})\n\
         stdout:\n{}\nstderr:\n{stderr}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
    );
}

/// (c) COLLATERAL GUARD. The tbb-pin dir holds EXACTLY one entry
/// (libtbb.so.12), so prepending it to RUNPATH must not redirect any other
/// soname. Assert none of these libs (when present in this bin's NEEDED
/// graph) resolve via the tbb-pin dir; they must stay on one of the
/// pre-existing RUNPATH entries (`/usr/lib/x86_64-linux-gnu` or
/// `/opt/reify-deps/lib`, depending on which native dep's rpath emission
/// listed it first — reify-cli's own OpenVDB rpath already lists
/// `/opt/reify-deps/lib` first, independent of this task).
#[test]
fn reify_binary_bare_launch_collateral_libs_do_not_shift_to_tbb_pin() {
    let exe = env!("CARGO_BIN_EXE_reify");
    if !tbb_pin_present() {
        eprintln!("{TBB_PIN_DIR}/libtbb.so.12 absent on this host; skipping collateral guard");
        return;
    }
    let Some(ldd_out) = bare_ldd(exe) else {
        return;
    };
    const COLLATERAL_LIBS: &[&str] = &[
        "libstdc++.so.6",
        "libgcc_s.so.1",
        "libglib-2.0.so.0",
        "libgobject-2.0.so.0",
        "libgio-2.0.so.0",
        "libcairo.so.2",
        "libgdk_pixbuf-2.0.so.0",
        "libdbus-1.so.3",
    ];
    for lib in COLLATERAL_LIBS {
        match ldd_resolved_path(&ldd_out, lib) {
            Some(resolved) => assert!(
                resolved.starts_with("/usr/lib/x86_64-linux-gnu/")
                    || resolved.starts_with("/opt/reify-deps/lib/"),
                "{lib} resolved to {resolved} — expected /usr/lib/x86_64-linux-gnu or \
                 /opt/reify-deps/lib (the pre-existing RUNPATH entries), never \
                 {TBB_PIN_DIR} (the tbb-only pin dir must flip no other soname)"
            ),
            None => eprintln!("{lib} not in {exe}'s NEEDED graph; skipping collateral check for it"),
        }
    }
}

/// (d) SECONDARY readelf guards (never the sole signal — see module doc).
/// The `reify` binary must carry a direct `NEEDED libtbb.so.12`, and
/// `/opt/reify-deps/tbb-pin` must be the FIRST entry in its RUNPATH.
#[test]
fn reify_binary_has_direct_needed_libtbb_and_pin_first_in_runpath() {
    let exe = env!("CARGO_BIN_EXE_reify");
    if !tbb_pin_present() {
        eprintln!("{TBB_PIN_DIR}/libtbb.so.12 absent on this host; skipping");
        return;
    }
    let Some(stdout) = readelf_d(exe) else {
        return;
    };
    let needed_tk: Vec<&str> =
        stdout.lines().filter(|l| l.contains("(NEEDED)") && l.contains("libTK")).collect();
    if needed_tk.is_empty() {
        eprintln!("no NEEDED libTK* entries in {exe}; OCCT likely stubbed — skipping");
        return;
    }

    let has_direct_tbb_needed =
        stdout.lines().any(|l| l.contains("(NEEDED)") && l.contains("[libtbb.so.12]"));
    assert!(
        has_direct_tbb_needed,
        "readelf -d {exe} has no direct NEEDED libtbb.so.12 entry — \
         emit_tbb_pin_for_bins()'s forced --no-as-needed -l:libtbb.so.12 link-arg \
         may be missing or not taking effect.\n\nFull readelf -d output:\n{stdout}"
    );

    let runpath_line = stdout
        .lines()
        .find(|l| l.contains("(RUNPATH)") || l.contains("(RPATH)"))
        .unwrap_or_else(|| panic!("no RUNPATH/RPATH entry in readelf -d {exe}:\n{stdout}"));
    let dirs: Vec<&str> = runpath_line
        .split('[')
        .nth(1)
        .and_then(|s| s.split(']').next())
        .unwrap_or_else(|| panic!("could not parse RUNPATH/RPATH entry: {runpath_line}"))
        .split(':')
        .collect();
    assert_eq!(
        dirs.first().copied(),
        Some(TBB_PIN_DIR),
        "{TBB_PIN_DIR} must be FIRST in {exe}'s RUNPATH so its direct NEEDED \
         libtbb.so.12 resolves there ahead of any transitive edge, got: {dirs:?}"
    );
}
