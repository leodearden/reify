//! Shared build-script helpers for Reify's native-dep wiring.
//!
//! Two responsibilities, consolidated here to prevent drift:
//!
//! 1. **Discovery** — locate the include/lib dirs for OCCT, Gmsh, and OpenVDB,
//!    honouring environment overrides and falling back to a canonical search
//!    list. Previously duplicated 3× across `reify-kernel-{occt,gmsh,openvdb}/
//!    build.rs`.
//!
//! 2. **RPATH propagation to binary packages** — Cargo's `rustc-link-arg`
//!    directive only applies to bins/tests in the *same* package emitting it,
//!    so RPATH directives from kernel adapter build.rs scripts do NOT reach
//!    the workspace binaries (`reify`, `reify-gui`) that transitively depend
//!    on those adapters. Binary packages must call [`emit_rpath_for_bins`]
//!    from their own build.rs to embed RPATH into their bin targets.
//!
//! See `crates/reify-cli/build.rs` and `gui/src-tauri/build.rs` for the
//! binary-side usage.

use std::env;
use std::path::{Path, PathBuf};

/// The native libraries Reify binaries may link against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeDep {
    Occt,
    Gmsh,
    OpenVdb,
}

/// Resolved location of a native library's headers and shared objects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibLoc {
    pub lib_dir: PathBuf,
    pub include_dir: PathBuf,
}

impl NativeDep {
    /// Env var that overrides the include search list, if set.
    fn include_env(self) -> &'static str {
        match self {
            NativeDep::Occt => "OCCT_INCLUDE_DIR",
            NativeDep::Gmsh => "GMSH_INCLUDE_DIR",
            NativeDep::OpenVdb => "OPENVDB_INCLUDE_DIR",
        }
    }

    /// Env var that overrides the lib search list, if set.
    fn lib_env(self) -> &'static str {
        match self {
            NativeDep::Occt => "OCCT_LIB_DIR",
            NativeDep::Gmsh => "GMSH_LIB_DIR",
            NativeDep::OpenVdb => "OPENVDB_LIB_DIR",
        }
    }

    /// Canonical include-dir candidates in priority order.
    ///
    /// The OCCT arm below is MIRRORED verbatim — order included — in the
    /// `occt-candidates` marker block of `scripts/check-manifold-deps.sh`,
    /// whose preflight has to reach the same verdict this function feeds into
    /// `has_occt`. This side stays the single source of truth; parity is
    /// pinned by `tests/infra/test_occt_deps_preflight.sh`, so an edit here
    /// without the matching bash edit fails that guard rather than silently
    /// leaving the gate and the build disagreeing.
    fn include_candidates(self) -> &'static [&'static str] {
        match self {
            NativeDep::Occt => &[
                "/usr/include/opencascade",
                "/usr/local/include/opencascade",
                "/snap/freecad/current/usr/include/opencascade",
            ],
            NativeDep::Gmsh => &["/opt/reify-deps/include", "/usr/include", "/usr/local/include"],
            NativeDep::OpenVdb => {
                &["/opt/reify-deps/include", "/usr/local/include", "/usr/include"]
            }
        }
    }

    /// Canonical lib-dir candidates in priority order.
    ///
    /// OCCT's list intentionally lists system paths *before* `/opt/reify-deps/lib`
    /// because the conda env ships OCCT 7.9 (a transitive dep of gmsh=4.15.2)
    /// while we want to link the system OCCT 7.8 — and gmsh/openvdb list
    /// `/opt/reify-deps/lib` first because that's where their canonical install
    /// lives via `scripts/setup-dev.sh`.
    ///
    /// That ordering is precisely what the bash mirror in
    /// `scripts/check-manifold-deps.sh`'s `occt-candidates` block must
    /// preserve, which is why `tests/infra/test_occt_deps_preflight.sh`
    /// compares the two lists order-sensitively. See the note on the
    /// include-dir candidates above.
    fn lib_candidates(self) -> &'static [&'static str] {
        match self {
            NativeDep::Occt => &[
                "/usr/lib/x86_64-linux-gnu",
                "/usr/lib",
                "/usr/local/lib",
                "/snap/freecad/current/usr/lib",
            ],
            NativeDep::Gmsh => &[
                "/opt/reify-deps/lib",
                "/usr/lib/x86_64-linux-gnu",
                "/usr/lib",
                "/usr/local/lib",
            ],
            NativeDep::OpenVdb => {
                &["/opt/reify-deps/lib", "/usr/local/lib", "/usr/lib/x86_64-linux-gnu", "/usr/lib"]
            }
        }
    }

    /// Sentinel header used to confirm an include-dir candidate.
    fn include_sentinel(self) -> &'static str {
        match self {
            NativeDep::Occt => "Standard_Failure.hxx",
            NativeDep::Gmsh => "gmshc.h",
            NativeDep::OpenVdb => "openvdb/openvdb.h",
        }
    }

    /// Sentinel shared-object name (canonical symlink) used to confirm a
    /// lib-dir candidate.
    fn lib_sentinel(self) -> &'static str {
        match self {
            NativeDep::Occt => "libTKernel.so",
            NativeDep::Gmsh => "libgmsh.so",
            NativeDep::OpenVdb => "libopenvdb.so",
        }
    }
}

/// Locate the include and lib dirs for `dep`. Honours the env-var override
/// (e.g. `OCCT_LIB_DIR`) when set; otherwise probes the canonical candidate
/// list and selects the first one containing the sentinel header / library.
/// Returns `None` if neither is found — kernel adapters use this to enter
/// stub-only mode without failing the build.
pub fn find(dep: NativeDep) -> Option<LibLoc> {
    let include_dir = find_dir(dep.include_env(), dep.include_candidates(), dep.include_sentinel());
    let lib_dir = find_dir(dep.lib_env(), dep.lib_candidates(), dep.lib_sentinel());
    match (include_dir, lib_dir) {
        (Some(include_dir), Some(lib_dir)) => Some(LibLoc { lib_dir, include_dir }),
        _ => None,
    }
}

fn find_dir(env_var: &str, candidates: &[&str], sentinel: &str) -> Option<PathBuf> {
    find_dir_with_override(env::var(env_var).ok().as_deref(), candidates, sentinel)
}

/// Resolution core of [`find_dir`] with the override passed in as a value, so
/// precedence is testable without `env::set_var` racing libtest's threads.
///
/// An EMPTY override counts as UNSET. `env::var(..).ok()` hands back
/// `Some("")` for an exported-but-empty var, and taking that on trust would
/// resolve the dir to the empty path, still set `has_occt`, and link against
/// nothing — while `scripts/check-manifold-deps.sh`'s OCCT preflight, reading
/// the same environment, fell through to the candidate list and went green
/// describing a resolution this function would not perform. Both halves of
/// that mirror now agree; the bash half is the `[ -n "$override" ]` test in
/// that script's `occt_find_dir`.
fn find_dir_with_override(
    override_dir: Option<&str>,
    candidates: &[&str],
    sentinel: &str,
) -> Option<PathBuf> {
    if let Some(dir) = override_dir.filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    for p in candidates {
        let path = PathBuf::from(p);
        if path.join(sentinel).exists() {
            return Some(path);
        }
    }
    // OCCT's snap fallback: numbered /snap/freecad/<rev>/ directories.
    let snap_subdir = match sentinel {
        "Standard_Failure.hxx" => Some("usr/include/opencascade"),
        "libTKernel.so" => Some("usr/lib"),
        _ => None,
    };
    if let Some(subdir) = snap_subdir
        && let Ok(entries) = std::fs::read_dir("/snap/freecad")
    {
        for entry in entries.flatten() {
            let candidate = entry.path().join(subdir);
            if candidate.join(sentinel).exists() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Probe for `dep`; if its lib_dir is found, emit
/// `cargo:rustc-link-arg-bins=-Wl,-rpath,<lib_dir>` so binaries defined in
/// the calling package embed RUNPATH for that directory. Returns whether the
/// directive was emitted.
///
/// This is the binary-package-side complement to the in-package
/// `rustc-link-arg=-Wl,-rpath,<lib_dir>` that kernel adapter build.rs scripts
/// emit for their own test bins: Cargo does not propagate `rustc-link-arg`
/// across package boundaries, so workspace binaries (`reify`, `reify-gui`)
/// would otherwise launch without RUNPATH and rely on the system ld.so cache.
pub fn emit_rpath_for_bins(dep: NativeDep) -> bool {
    println!("cargo:rerun-if-env-changed={}", dep.lib_env());
    if let Some(lib_dir) = find_dir(dep.lib_env(), dep.lib_candidates(), dep.lib_sentinel()) {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,{}", lib_dir.display());
        true
    } else {
        false
    }
}

/// Probe for `dep`; if its lib_dir is found, emit unscoped
/// `cargo:rustc-link-arg=-Wl,-rpath,<lib_dir>` so every supported build
/// target (bins, examples, integration tests, **and lib-unittests**) in
/// the calling package embeds RUNPATH for that directory.
///
/// Needed for packages whose own test binaries transitively link a native
/// lib — either via a normal dep (`reify-solver-elastic` → gmsh) or a
/// dev-dep (`reify-config` → all kernels). The narrower
/// `rustc-link-arg-tests=...` directive does **not** apply to the lib
/// unittests binary produced by `cargo test --lib`, so we use the
/// unscoped form which covers it.
///
/// For packages with bins of their own (`reify-cli`, `reify-gui`), this
/// also applies to those bins; that's identical in effect to
/// [`emit_rpath_for_bins`] and harmless when both are called.
pub fn emit_rpath_for_tests(dep: NativeDep) -> bool {
    println!("cargo:rerun-if-env-changed={}", dep.lib_env());
    if let Some(lib_dir) = find_dir(dep.lib_env(), dep.lib_candidates(), dep.lib_sentinel()) {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());
        true
    } else {
        false
    }
}

/// Absolute path to the tbb-only RUNPATH pin dir materialized by
/// `scripts/build-manifold-deps.sh` (task #5192, mechanism A″). Contains
/// EXACTLY one entry — a `libtbb.so.12` symlink chaining to the deps lib —
/// so prepending it to RUNPATH flips no other soname.
const TBB_PIN_DIR: &str = "/opt/reify-deps/tbb-pin";

fn tbb_pin_lib() -> PathBuf {
    Path::new(TBB_PIN_DIR).join("libtbb.so.12")
}

/// Probe for the tbb-pin dir; if present, emit directives so the calling
/// package's bin targets carry a DIRECT `NEEDED libtbb.so.12`, resolved via
/// [`TBB_PIN_DIR`] prepended FIRST in RUNPATH. Fail-soft: no-op (returns
/// `false`, mirroring [`find_dir`]'s posture) when the pin dir is absent,
/// so stub-only builds (no `/opt/reify-deps` host) still compile.
///
/// Mechanism A″ (task #5192): a workspace binary's own RUNPATH can never
/// redirect the *transitive* NEEDED `libtbb.so.12` pulled in by system
/// `libTKernel.so` (OCCT) — DT_RUNPATH is non-transitive. Giving the binary
/// its OWN direct NEEDED — resolved via this tbb-only pin dir, which the
/// loader consults for the binary's own direct deps — lets it load the
/// deps `libtbb.so.12.18` (which has symbols the system `libtbb.so.12`
/// lacks) *before* the transitive `libTKernel` edge in the loader's BFS, so
/// every later `libtbb.so.12` soname need binds the same already-loaded
/// copy.
///
/// Call this BEFORE [`emit_rpath_for_bins`] in the binary package's
/// `build.rs` so the tbb-pin `-rpath` is emitted first and lands first in
/// DT_RUNPATH (`ld` concatenates `-rpath` occurrences in command-line
/// order).
pub fn emit_tbb_pin_for_bins() -> bool {
    emit_tbb_pin("cargo:rustc-link-arg-bins")
}

/// Same as [`emit_tbb_pin_for_bins`] but emits the unscoped
/// `cargo:rustc-link-arg` form so test/example/lib-unittest bins are
/// covered too — mirrors [`emit_rpath_for_tests`].
///
/// For packages with bins of their own (`reify-cli`, `reify-gui`) that call
/// both this and [`emit_tbb_pin_for_bins`], the bin target receives the
/// pin `-rpath` and the forced `--no-as-needed -l:libtbb.so.12 --as-needed`
/// link-arg twice (once bin-scoped, once unscoped) — this is intentional,
/// the same bin double-emission [`emit_rpath_for_tests`] documents for
/// `-rpath` alone. A duplicate `-rpath` token is harmlessly idempotent; a
/// duplicate direct `NEEDED libtbb.so.12` entry resolves identically at
/// load time (both bind the same pin-dir copy) and is harmless to the
/// loader, just slightly redundant in the ELF.
pub fn emit_tbb_pin_for_tests() -> bool {
    emit_tbb_pin("cargo:rustc-link-arg")
}

fn emit_tbb_pin(link_arg: &str) -> bool {
    let pin_lib = tbb_pin_lib();
    // Re-run if the pin symlink is created/changed later, even in the
    // fail-soft no-op case below.
    println!("cargo:rerun-if-changed={}", pin_lib.display());
    if !pin_lib.exists() {
        return false;
    }
    // Pin dir FIRST in RUNPATH so the exe's own direct NEEDED resolves
    // there ahead of any transitive libtbb.so.12 need.
    println!("{link_arg}=-Wl,-rpath,{TBB_PIN_DIR}");
    // Link-time resolution for the `-l:libtbb.so.12` directive below.
    println!("cargo:rustc-link-search=native={TBB_PIN_DIR}");
    // The exe's own objects reference no tbb symbol, so plain --as-needed
    // (rustc's default posture) would drop the NEEDED entry; force it.
    // `-l:libtbb.so.12` records the file's SONAME, not an absolute path,
    // so it stays a normal soname need resolvable via RUNPATH.
    println!("{link_arg}=-Wl,--no-as-needed,-l:libtbb.so.12,--as-needed");
    true
}

/// Read the SONAME suffix encoded into the canonical symlink for `lib_name`
/// at `lib_dir`. Used by `reify-kernel-occt/build.rs` to pin OCCT linkage to
/// the exact filename that exists at the resolved `lib_dir` (e.g.
/// `:libTKernel.so.7.8`) — defending against conda-forge's
/// `/opt/reify-deps/lib` (which ships OCCT 7.9 as a transitive dep of gmsh)
/// shadowing system OCCT 7.8.
///
/// Returns the trailing version segment (everything after `lib<name>.so.`),
/// e.g. `"7.8"` on a system where `libTKernel.so → libTKernel.so.7.8`. Returns
/// `None` if the symlink is missing, unreadable, or has no version suffix —
/// callers fall back to a hard-coded default.
pub fn read_soname_version(lib_dir: &Path, lib_name: &str) -> Option<String> {
    let canonical = lib_dir.join(format!("lib{lib_name}.so"));
    let target = std::fs::read_link(&canonical).ok()?;
    let target_name = target.file_name()?.to_str()?;
    let prefix = format!("lib{lib_name}.so.");
    target_name.strip_prefix(&prefix).map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;

    #[test]
    fn read_soname_version_extracts_trailing_segment() {
        let guard = tempdir();
        let tmp = guard.path();
        let real = tmp.join("libTKernel.so.7.8.1");
        fs::write(&real, b"").unwrap();
        symlink("libTKernel.so.7.8.1", tmp.join("libTKernel.so.7.8")).unwrap();
        symlink("libTKernel.so.7.8", tmp.join("libTKernel.so")).unwrap();

        // First-level symlink target is `libTKernel.so.7.8` → version "7.8".
        assert_eq!(read_soname_version(tmp, "TKernel"), Some("7.8".to_string()));
    }

    #[test]
    fn read_soname_version_returns_none_for_missing_symlink() {
        let guard = tempdir();
        let tmp = guard.path();
        assert_eq!(read_soname_version(tmp, "TKernel"), None);
    }

    #[test]
    fn read_soname_version_handles_conda_one_level_symlink() {
        let guard = tempdir();
        let tmp = guard.path();
        let real = tmp.join("libTKernel.so.7.9.3");
        fs::write(&real, b"").unwrap();
        symlink("libTKernel.so.7.9.3", tmp.join("libTKernel.so")).unwrap();

        // Conda-forge layout: `libTKernel.so → libTKernel.so.7.9.3` directly.
        // We extract the trailing segment verbatim (`"7.9.3"`); a `:lib...` link
        // directive built from this value matches the exact file on disk.
        assert_eq!(read_soname_version(tmp, "TKernel"), Some("7.9.3".to_string()));
    }

    /// A non-empty override outranks a candidate that would otherwise match,
    /// and is taken on trust — it is returned even without the sentinel present,
    /// which is what lets an operator point a build at a lib dir we do not
    /// know about.
    #[test]
    fn find_dir_override_takes_precedence_over_candidates() {
        let override_guard = tempdir();
        let override_dir = override_guard.path();

        // A candidate that DOES contain the sentinel, so the assertion below
        // can only pass if the override actually outranks it.
        let candidate_guard = tempdir();
        let candidate_dir = candidate_guard.path();
        fs::write(candidate_dir.join("libgmsh.so"), b"").unwrap();

        let override_str = override_dir.to_string_lossy().into_owned();
        let candidate_str = candidate_dir.to_string_lossy().into_owned();
        let found = find_dir_with_override(
            Some(override_str.as_str()),
            &[candidate_str.as_str()],
            "libgmsh.so",
        );

        assert_eq!(found.as_deref(), Some(override_dir));
    }

    /// An exported-but-EMPTY override counts as unset here, matching the
    /// `[ -n "$override" ]` half of the mirror in
    /// `scripts/check-manifold-deps.sh`'s `occt_find_dir`. Without the filter
    /// this resolved to the empty path and set `has_occt` while the preflight,
    /// reading the same environment, went green — exactly the guard/build
    /// disagreement that arm exists to prevent.
    #[test]
    fn find_dir_ignores_exported_but_empty_override() {
        let guard = tempdir();
        let tmp = guard.path();
        fs::write(tmp.join("libgmsh.so"), b"").unwrap();

        let tmp_str = tmp.to_string_lossy().into_owned();
        let found = find_dir_with_override(Some(""), &[tmp_str.as_str()], "libgmsh.so");

        assert_eq!(
            found.as_deref(),
            Some(tmp),
            "empty override must fall through"
        );
    }

    #[test]
    fn find_dir_falls_through_candidates_when_env_unset() {
        let guard = tempdir();
        let tmp = guard.path();
        let sentinel = tmp.join("libgmsh.so");
        fs::write(&sentinel, b"").unwrap();

        let tmp_str = tmp.to_string_lossy().into_owned();
        let candidates: Vec<&str> = vec!["/definitely/does/not/exist", tmp_str.as_str()];
        let found = find_dir("REIFY_BUILD_UTILS_TEST_UNSET_VAR", &candidates, "libgmsh.so");
        assert_eq!(found.as_deref(), Some(tmp));
    }

    /// Printed by the child below only after it asserts, so the parent can
    /// tell a real pass from libtest's filter matching zero tests.
    const CHILD_ASSERTED: &str = "reify-build-utils: child asserted";

    /// Covers what the seam-level test above deliberately bypasses: `find_dir`
    /// reads the env var it is named, and `find` routes each override to the
    /// matching `LibLoc` slot. No-op unless re-exec'd by the parent below,
    /// which supplies both vars at spawn.
    #[test]
    fn find_gmsh_honours_env_overrides_child() {
        let (Ok(include_dir), Ok(lib_dir)) =
            (env::var("GMSH_INCLUDE_DIR"), env::var("GMSH_LIB_DIR"))
        else {
            return;
        };

        let found = find(NativeDep::Gmsh).expect("both overrides set, so both dirs resolve");
        assert_eq!(found.include_dir, PathBuf::from(include_dir));
        assert_eq!(found.lib_dir, PathBuf::from(lib_dir));
        println!("{CHILD_ASSERTED}");
    }

    /// Re-execs this test binary with the overrides set in the child's spawn
    /// environment; `env::set_var` in-process would race libtest's threads.
    #[test]
    fn find_gmsh_honours_env_overrides() {
        let include_guard = tempdir();
        let lib_guard = tempdir();

        let exe = env::current_exe().expect("path to this test binary");
        let out = std::process::Command::new(exe)
            .args(["--exact", "tests::find_gmsh_honours_env_overrides_child", "--nocapture"])
            .env("GMSH_INCLUDE_DIR", include_guard.path())
            .env("GMSH_LIB_DIR", lib_guard.path())
            .output()
            .expect("re-exec this test binary");

        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "child test failed ({}):\n{stdout}", out.status);
        assert!(
            stdout.contains(CHILD_ASSERTED),
            "child never reached its assertions (filter matched no test?):\n{stdout}"
        );
    }

    /// Canary against a future rewrite of `tempdir()` that leaks again.
    #[test]
    fn tempdir_removes_directory_on_scope_exit() {
        let observed: PathBuf;
        {
            let guard = tempdir();
            observed = guard.path().to_path_buf();
        } // guard dropped here

        assert!(
            !observed.exists(),
            "temp dir leaked after scope exit: {}",
            observed.display()
        );
    }

    /// Removed on drop; the `reify-build-utils-test-` prefix keeps debris from
    /// a SIGKILL (which `Drop` cannot cover) attributable to this crate.
    /// Bind the guard to a named local — `tempdir().path().to_path_buf()`
    /// compiles but deletes the dir at the end of that statement.
    fn tempdir() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("reify-build-utils-test-")
            .tempdir()
            .expect("create temp dir")
    }
}
