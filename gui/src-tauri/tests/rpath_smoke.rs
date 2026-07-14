//! Pin: the `reify-gui` workspace binary embeds a RUNPATH entry and does
//! NOT link conda-forge OCCT 7.9. Mirrors
//! `crates/reify-cli/tests/rpath_smoke.rs`; see its module doc for full
//! rationale. Only runs when the `gui` feature is enabled (the binary
//! requires it via `required-features`).

#![cfg(all(target_os = "linux", feature = "gui"))]

use std::process::Command;

#[test]
fn reify_gui_binary_embeds_runpath() {
    let exe = env!("CARGO_BIN_EXE_reify-gui");
    let Some(stdout) = readelf_d(exe) else {
        return;
    };
    let has_rpath = stdout.contains("(RPATH)") || stdout.contains("(RUNPATH)");
    assert!(
        has_rpath,
        "readelf -d {exe} produced no (RPATH) or (RUNPATH) entry — \
         gui/src-tauri/build.rs may have regressed and dropped its \
         emit_rpath_for_bins call.\n\nFull readelf -d output:\n{stdout}"
    );
}

#[test]
fn reify_gui_binary_does_not_link_conda_occt_7_9() {
    let exe = env!("CARGO_BIN_EXE_reify-gui");
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
        "{exe} NEEDS conda-forge OCCT 7.9 libs:\n{leaks}\n\nFull NEEDED \
         libTK* lines:\n{all}",
        leaks = leaks_7_9.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n"),
        all = needed_tk.join("\n"),
    );
}

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
        eprintln!("readelf -d exited non-zero (status={:?}); skipping", output.status);
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

// ---------------------------------------------------------------------
// Mechanism A'' (task #5192): tbb-only RUNPATH pin dir + forced direct
// `NEEDED libtbb.so.12`, proven at RUNTIME (not just readelf token order —
// DT_RUNPATH's non-transitivity makes a token-order-only check phantom
// green). Mirrors crates/reify-cli/tests/rpath_smoke.rs; see its module
// doc for full rationale. No GUI bare-launch smoke here — the windowed
// launch + MCP health check is the verify skill's job (acceptance #4).
// ---------------------------------------------------------------------

const TBB_PIN_DIR: &str = "/opt/reify-deps/tbb-pin";

/// Whether this host has the tbb-pin dir materialized
/// (`scripts/build-manifold-deps.sh`).
fn tbb_pin_present() -> bool {
    std::path::Path::new(TBB_PIN_DIR).join("libtbb.so.12").exists()
}

/// Run `env -u LD_LIBRARY_PATH ldd <path>` and return its stdout. `None`
/// (skip) when `env`/`ldd` is unavailable or the invocation itself fails.
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
/// `<soname> => <path> (0x...)`. `None` if `soname` has no such line.
fn ldd_resolved_path<'a>(ldd_output: &'a str, soname: &str) -> Option<&'a str> {
    let needle = format!("{soname} =>");
    ldd_output
        .lines()
        .find_map(|line| line.trim().strip_prefix(needle.as_str())?.split_whitespace().next())
}

/// (a) RUNTIME RESOLUTION (primary). `env -u LD_LIBRARY_PATH ldd
/// <reify-gui>` must resolve `libtbb.so.12` to the deps copy —
/// canonicalized, so it tolerates the tbb-pin symlink hop.
#[test]
fn reify_gui_binary_ldd_resolves_libtbb_to_deps_lib() {
    let exe = env!("CARGO_BIN_EXE_reify-gui");
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
         its RUNPATH (gui/src-tauri/build.rs may be missing the emit_tbb_pin_for_bins() \
         call)."
    );
}

/// (b) COLLATERAL GUARD across the full GTK/webkit stack `reify-gui`
/// links: the tbb-pin dir holds EXACTLY one entry, so none of these (when
/// present) may resolve via it.
#[test]
fn reify_gui_binary_collateral_libs_do_not_shift_to_tbb_pin() {
    let exe = env!("CARGO_BIN_EXE_reify-gui");
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

/// (c) SECONDARY readelf guards (never the sole signal — see module doc).
/// `reify-gui` must carry a direct `NEEDED libtbb.so.12`, and
/// `/opt/reify-deps/tbb-pin` must be FIRST in its RUNPATH.
#[test]
fn reify_gui_binary_has_direct_needed_libtbb_and_pin_first_in_runpath() {
    let exe = env!("CARGO_BIN_EXE_reify-gui");
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
