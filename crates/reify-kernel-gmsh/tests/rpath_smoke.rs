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
//! The RPATH-pin test (`compiled_test_binary_embeds_rpath_to_reify_deps_lib`)
//! is compiled only when both `cfg(has_gmsh)` (libgmsh detected at build
//! time → RPATH directive issued) AND `target_os = "linux"` (readelf is
//! a binutils tool; macOS uses `otool -l`, Windows has no equivalent —
//! we don't claim cross-platform RPATH coverage in v0.3).
//!
//! The synthetic isolation test
//! (`direct_needed_binds_pinned_leaf_over_transitive_system`, task #5192)
//! has ZERO dependency on gmsh — it builds its own C exe/mid.so/leaf.so
//! trio — so the module gate is just `target_os = "linux"`, with the
//! gmsh-specific test additionally gated on `cfg(has_gmsh)` so
//! `env!("REIFY_GMSH_LIB_DIR")` (only emitted by build.rs when gmsh is
//! found) keeps compiling correctly on stub-only builds.

#![cfg(target_os = "linux")]

use std::path::PathBuf;
use std::process::Command;

/// Pin the RPATH/RUNPATH directive emitted by `build.rs` (step pre-2).
///
/// Invokes `readelf -d` on the test binary itself; asserts the output
/// references the lib_dir `build.rs` actually used (read via `env!` from
/// the `REIFY_GMSH_LIB_DIR` `cargo:rustc-env=` directive). If `readelf`
/// is not on PATH (e.g. minimal CI image), skip with a stderr note
/// rather than failing — mirrors the OCCT-availability skip pattern.
#[cfg(has_gmsh)]
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

// ---------------------------------------------------------------------
// Synthetic isolation test (task #5192, mechanism A''): a self-contained,
// stack-independent proof of the "direct NEEDED loads first" invariant
// that the tbb-only RUNPATH pin dir relies on. Builds its own
// exe -> mid.so -> leaf.so trio (with a same-soname leaf duplicated into
// two dirs with different return values) using `cc`, entirely independent
// of gmsh/OCCT/tbb. Kept as a permanent regression guard.
// ---------------------------------------------------------------------

fn cc_path() -> Option<&'static str> {
    ["cc", "gcc"].into_iter().find(|c| Command::new(c).arg("--version").output().is_ok())
}

fn synthetic_tempdir() -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().subsec_nanos();
    let base = std::env::temp_dir()
        .join(format!("reify-tbb-pin-synthetic-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&base).expect("create synthetic tempdir");
    base
}

/// RAII guard that removes the synthetic tempdir on drop — including on a
/// panic-unwind from an earlier `assert!`/`run_cc`/`run_bare` failure — so
/// per-pid tempdirs under `/tmp` don't leak and accumulate across repeated
/// CI runs when the test fails partway through. `Deref`s to `Path` so call
/// sites can use it exactly like the `PathBuf` it wraps.
struct TempDirGuard(PathBuf);

impl std::ops::Deref for TempDirGuard {
    type Target = std::path::Path;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run_cc(cc: &str, args: &[&str]) {
    let output = Command::new(cc).args(args).output().expect("spawn cc");
    assert!(
        output.status.success(),
        "`{cc} {args:?}` failed (status={:?}):\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn run_bare(exe: &std::path::Path) -> String {
    let output = Command::new("env")
        .args(["-u", "LD_LIBRARY_PATH"])
        .arg(exe)
        .output()
        .unwrap_or_else(|e| panic!("spawn {}: {e}", exe.display()));
    assert!(
        output.status.success(),
        "{} exited non-zero (status={:?}):\nstdout:\n{}\nstderr:\n{}",
        exe.display(),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Proves the OS-loader premise mechanism A'' relies on: a direct NEEDED,
/// resolved via a dir prepended FIRST in the exe's own RUNPATH, loads
/// ahead of a transitive dependency's own (differently-ordered) need for
/// the SAME soname — so the transitive edge ends up binding the same
/// already-loaded (pinned) copy via soname dedup, regardless of what the
/// transitively-loaded library's own RUNPATH would otherwise resolve to.
#[test]
fn direct_needed_binds_pinned_leaf_over_transitive_system() {
    let Some(cc) = cc_path() else {
        eprintln!("no C compiler (cc/gcc) on PATH; skipping synthetic isolation test");
        return;
    };

    let root = TempDirGuard(synthetic_tempdir());
    let pin_dir = root.join("pin");
    let sys_dir = root.join("sys");
    let mid_dir = root.join("midlib");
    for d in [&pin_dir, &sys_dir, &mid_dir] {
        std::fs::create_dir_all(d).expect("create synthetic subdir");
    }

    // Same soname in two dirs, different content: pin/ returns 1, sys/
    // returns 2.
    let leaf_pin_src = root.join("leaf_pin.c");
    std::fs::write(&leaf_pin_src, "int leaf_which(void) { return 1; }\n").unwrap();
    let leaf_pin_so = pin_dir.join("libleaf_synthetic.so.1");
    run_cc(
        cc,
        &[
            "-shared",
            "-fPIC",
            "-Wl,-soname,libleaf_synthetic.so.1",
            "-o",
            leaf_pin_so.to_str().unwrap(),
            leaf_pin_src.to_str().unwrap(),
        ],
    );

    let leaf_sys_src = root.join("leaf_sys.c");
    std::fs::write(&leaf_sys_src, "int leaf_which(void) { return 2; }\n").unwrap();
    let leaf_sys_so = sys_dir.join("libleaf_synthetic.so.1");
    run_cc(
        cc,
        &[
            "-shared",
            "-fPIC",
            "-Wl,-soname,libleaf_synthetic.so.1",
            "-o",
            leaf_sys_so.to_str().unwrap(),
            leaf_sys_src.to_str().unwrap(),
        ],
    );

    // mid.so calls leaf_which(); linked against the SYSTEM leaf via its
    // OWN rpath — establishes the non-transitive baseline (mid resolves
    // its own dependency via its own rpath, independent of any caller).
    let mid_src = root.join("mid.c");
    std::fs::write(
        &mid_src,
        "int leaf_which(void);\nint mid_call(void) { return leaf_which(); }\n",
    )
    .unwrap();
    let mid_so = mid_dir.join("libmid_synthetic.so");
    run_cc(
        cc,
        &[
            "-shared",
            "-fPIC",
            "-o",
            mid_so.to_str().unwrap(),
            mid_src.to_str().unwrap(),
            &format!("-L{}", sys_dir.display()),
            "-l:libleaf_synthetic.so.1",
            &format!("-Wl,-rpath,{}", sys_dir.display()),
        ],
    );

    // Control: an exe linking ONLY mid.so (no pin-dir trick) must observe
    // the system leaf (2) — proves the harness's baseline matches
    // standard non-transitive RUNPATH semantics before testing the fix.
    let probe_src = root.join("mid_probe.c");
    std::fs::write(
        &probe_src,
        "#include <stdio.h>\nint mid_call(void);\nint main(void) { printf(\"%d\\n\", mid_call()); return 0; }\n",
    )
    .unwrap();
    let probe_exe = root.join("mid_probe");
    run_cc(
        cc,
        &[
            probe_src.to_str().unwrap(),
            "-o",
            probe_exe.to_str().unwrap(),
            &format!("-L{}", mid_dir.display()),
            "-lmid_synthetic",
            &format!("-Wl,-rpath,{}", mid_dir.display()),
        ],
    );
    assert_eq!(
        run_bare(&probe_exe),
        "2",
        "control failed: mid.so alone should resolve leaf_which via its own \
         rpath to the system copy (2) — synthetic harness does not match \
         standard non-transitive RUNPATH semantics"
    );

    // Main proof: exe calls mid_call() AND carries a FORCED direct NEEDED
    // on the same soname (mirroring mechanism A'': the real exe's own
    // objects reference no tbb symbol either), resolved via the pin dir
    // prepended FIRST in its own RUNPATH.
    let exe_src = root.join("exe.c");
    std::fs::write(
        &exe_src,
        "#include <stdio.h>\nint mid_call(void);\nint main(void) { printf(\"%d\\n\", mid_call()); return 0; }\n",
    )
    .unwrap();
    let exe = root.join("exe");
    run_cc(
        cc,
        &[
            exe_src.to_str().unwrap(),
            "-o",
            exe.to_str().unwrap(),
            &format!("-L{}", mid_dir.display()),
            "-lmid_synthetic",
            &format!("-L{}", pin_dir.display()),
            "-Wl,--no-as-needed,-l:libleaf_synthetic.so.1,--as-needed",
            &format!(
                "-Wl,-rpath,{}:{}:{}",
                pin_dir.display(),
                mid_dir.display(),
                sys_dir.display()
            ),
        ],
    );
    assert_eq!(
        run_bare(&exe),
        "1",
        "exe's direct NEEDED (resolved via the pin dir first in RUNPATH) did \
         not win over mid.so's transitive need for the system copy — the \
         direct-NEEDED-loads-first invariant underpinning mechanism A'' does \
         not hold on this host/loader"
    );

    // `root`'s `TempDirGuard` removes the tempdir on drop here (and would
    // have on an earlier panic-unwind too).
}
