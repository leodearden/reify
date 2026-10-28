//! Shared build-script helpers for Reify's native-dep wiring.
//!
//! Two responsibilities, consolidated here to prevent drift:
//!
//! 1. **Discovery** — locate the include/lib dirs for OCCT, Gmsh, and OpenVDB,
//!    honouring environment overrides and falling back to each dep's ordered
//!    candidate list. Each dep's discovery data is its row in
//!    `native-deps.toml`, decoded once.
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

use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

/// Declares [`NativeDep`], its variant-to-row mapping and, for the tests, its
/// variant list, all from one list, so no variant can be left out of any of
/// the three.
macro_rules! native_deps {
    ($($variant:ident => $row_key:literal,)+) => {
        /// The native libraries Reify binaries may link against.
        ///
        /// Each variant's discovery data is its row in `native-deps.toml`,
        /// keyed by [`NativeDep::row_key`]; that file's header names its
        /// readers.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum NativeDep {
            $($variant,)+
        }

        impl NativeDep {
            #[cfg(test)]
            const ALL: &'static [NativeDep] = &[$(NativeDep::$variant,)+];

            /// The variant's row key in `native-deps.toml`: the one
            /// variant-to-row mapping.
            fn row_key(self) -> &'static str {
                match self {
                    $(NativeDep::$variant => $row_key,)+
                }
            }
        }
    };
}

native_deps! {
    Occt => "occt",
    Gmsh => "gmsh",
    OpenVdb => "openvdb",
}

/// Resolved location of a native library's headers and shared objects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibLoc {
    pub lib_dir: PathBuf,
    pub include_dir: PathBuf,
}

/// Repo-relative path of the table, for panic messages.
const TABLE_FILE: &str = "crates/reify-build-utils/native-deps.toml";
const TABLE_TOML: &str = include_str!("../native-deps.toml");

/// One `native-deps.toml` row.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DepRow {
    include_env: String,
    lib_env: String,
    cfg: String,
    include_candidates: Vec<String>,
    lib_candidates: Vec<String>,
    include_sentinel: String,
    lib_sentinel: String,
    snap: Option<SnapFallback>,
    #[serde(default)]
    xde_toolkits: Vec<String>,
}

/// Numbered `<root>/<rev>/<subdir>` installs, scanned after the candidates.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapFallback {
    root: String,
    include_subdir: String,
    lib_subdir: String,
}

type Table = BTreeMap<String, DepRow>;

/// The table is repo-owned data, so a malformed one is a bug rather than an
/// environment: panic naming the file.
fn parse_table(text: &str) -> Table {
    let table: Table = toml::from_str(text).unwrap_or_else(|err| panic!("{TABLE_FILE}: {err}"));
    for (key, row) in &table {
        if let Err(defect) = check_row(key, row) {
            panic!("{TABLE_FILE}: [{key}] {defect}");
        }
    }
    table
}

/// The row invariants the table's header states, which
/// `scripts/native_deps_table.py` enforces too.
fn check_row(key: &str, row: &DepRow) -> Result<(), String> {
    if !is_row_key(key) {
        return Err("row key is not [a-z][a-z0-9]*".to_string());
    }
    for (column, value) in [
        ("include_env", &row.include_env),
        ("lib_env", &row.lib_env),
        ("cfg", &row.cfg),
    ] {
        if !is_identifier(value) {
            return Err(format!("{column} {value:?} is not an identifier"));
        }
    }
    for (column, value) in [
        ("include_sentinel", &row.include_sentinel),
        ("lib_sentinel", &row.lib_sentinel),
    ] {
        if value.is_empty() {
            return Err(format!("{column} is empty"));
        }
    }
    Ok(())
}

fn is_row_key(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| parse_table(TABLE_TOML))
}

fn lookup<'t>(table: &'t Table, key: &str) -> &'t DepRow {
    table
        .get(key)
        .unwrap_or_else(|| panic!("{TABLE_FILE}: no [{key}] row"))
}

/// Which half of a dep a lookup is about.
#[derive(Clone, Copy)]
enum DirKind {
    Include,
    Lib,
}

impl DepRow {
    fn env(&self, kind: DirKind) -> &str {
        match kind {
            DirKind::Include => &self.include_env,
            DirKind::Lib => &self.lib_env,
        }
    }

    fn candidates(&self, kind: DirKind) -> &[String] {
        match kind {
            DirKind::Include => &self.include_candidates,
            DirKind::Lib => &self.lib_candidates,
        }
    }

    fn sentinel(&self, kind: DirKind) -> &str {
        match kind {
            DirKind::Include => &self.include_sentinel,
            DirKind::Lib => &self.lib_sentinel,
        }
    }
}

impl SnapFallback {
    fn subdir(&self, kind: DirKind) -> &str {
        match kind {
            DirKind::Include => &self.include_subdir,
            DirKind::Lib => &self.lib_subdir,
        }
    }
}

impl NativeDep {
    fn row(self) -> &'static DepRow {
        lookup(table(), self.row_key())
    }

    /// The toolkits `reify-kernel-occt` links for the STEP assembly reader
    /// after its base list: the row's `xde_toolkits`, empty when it lists none.
    pub fn xde_toolkits(self) -> &'static [String] {
        &self.row().xde_toolkits
    }
}

/// Locate the include and lib dirs for `dep`. Honours the env-var override
/// (e.g. `OCCT_LIB_DIR`) when set; otherwise probes the canonical candidate
/// list and selects the first one containing the sentinel header / library.
/// Returns `None` if neither is found — kernel adapters use this to enter
/// stub-only mode without failing the build.
pub fn find(dep: NativeDep) -> Option<LibLoc> {
    let row = dep.row();
    let include_dir = find_dir(row, DirKind::Include);
    let lib_dir = find_dir(row, DirKind::Lib);
    match (include_dir, lib_dir) {
        (Some(include_dir), Some(lib_dir)) => Some(LibLoc { lib_dir, include_dir }),
        _ => None,
    }
}

fn find_dir(row: &DepRow, kind: DirKind) -> Option<PathBuf> {
    resolve_dir(env::var(row.env(kind)).ok().as_deref(), row, kind)
}

/// Resolution core of [`find_dir`] with the override passed in as a value, so
/// precedence is testable without `env::set_var` racing libtest's threads.
///
/// A non-empty override wins, taken on trust. An EMPTY one counts as UNSET:
/// `env::var(..).ok()` hands back `Some("")` for an exported-but-empty var,
/// and trusting it would resolve the dir to the empty path, still set
/// `has_<dep>`, and link against nothing. Otherwise the first candidate
/// holding the sentinel wins, then — only for a row with a snap table — the
/// first `<root>/<entry>/<subdir>` holding it.
fn resolve_dir(override_dir: Option<&str>, row: &DepRow, kind: DirKind) -> Option<PathBuf> {
    if let Some(dir) = override_dir.filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let sentinel = row.sentinel(kind);
    let holds_sentinel = |dir: &Path| dir.join(sentinel).exists();
    if let Some(dir) = row
        .candidates(kind)
        .iter()
        .map(PathBuf::from)
        .find(|dir| holds_sentinel(dir))
    {
        return Some(dir);
    }
    let snap = row.snap.as_ref()?;
    std::fs::read_dir(&snap.root)
        .ok()?
        .flatten()
        .map(|entry| entry.path().join(snap.subdir(kind)))
        .find(|dir| holds_sentinel(dir))
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
    let row = dep.row();
    println!("cargo:rerun-if-env-changed={}", row.lib_env);
    if let Some(lib_dir) = find_dir(row, DirKind::Lib) {
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
    let row = dep.row();
    println!("cargo:rerun-if-env-changed={}", row.lib_env);
    if let Some(lib_dir) = find_dir(row, DirKind::Lib) {
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
/// `false`, the same posture as [`find`] returning `None`) when the pin dir
/// is absent, so stub-only builds (no `/opt/reify-deps` host) still compile.
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
/// covered too, as [`emit_rpath_for_tests`] does for the lib dir.
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
    use std::collections::BTreeSet;
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

    #[test]
    fn variant_set_equals_table_row_set() {
        let table = parse_table(TABLE_TOML);
        let table_rows: BTreeSet<&str> = table.keys().map(String::as_str).collect();
        let variant_rows: BTreeSet<&str> = NativeDep::ALL.iter().map(|dep| dep.row_key()).collect();
        assert_eq!(
            variant_rows.len(),
            NativeDep::ALL.len(),
            "two variants share a row key"
        );
        assert_eq!(table_rows, variant_rows);

        for dep in NativeDep::ALL {
            dep.row(); // panics naming the file if the row is missing
        }
    }

    /// Every kernel build.rs and every `#[cfg(..)]` gate is written against
    /// `has_<row key>`, so a `cfg` value drifting from it would name a cfg the
    /// build never sets.
    #[test]
    fn every_row_cfg_is_has_row_key() {
        for dep in NativeDep::ALL {
            assert_eq!(dep.row().cfg, format!("has_{}", dep.row_key()));
        }
    }

    #[test]
    #[should_panic(expected = "native-deps.toml: [open-vdb] row key")]
    fn row_key_outside_lowercase_alnum_panics_naming_the_file() {
        parse_table(&minimal_row_toml("occt").replace("[occt]", "[open-vdb]"));
    }

    #[test]
    #[should_panic(expected = "native-deps.toml: [occt] lib_env \"OCCT LIB\" is not an identifier")]
    fn non_identifier_env_panics_naming_the_file() {
        parse_table(&minimal_row_toml("occt").replace(
            "lib_env = \"REIFY_BUILD_UTILS_TEST_UNSET_VAR\"",
            "lib_env = \"OCCT LIB\"",
        ));
    }

    #[test]
    #[should_panic(expected = "native-deps.toml: [occt] cfg \"has-occt\" is not an identifier")]
    fn non_identifier_cfg_panics_naming_the_file() {
        parse_table(&minimal_row_toml("occt").replace("has_occt", "has-occt"));
    }

    /// An empty sentinel would let any existing candidate dir resolve.
    #[test]
    #[should_panic(expected = "native-deps.toml: [occt] include_sentinel is empty")]
    fn empty_sentinel_panics_naming_the_file() {
        parse_table(&minimal_row_toml("occt").replace("\"oc.h\"", "\"\""));
    }

    #[test]
    #[should_panic(expected = "native-deps.toml")]
    fn malformed_table_panics_naming_the_file() {
        parse_table("[occt");
    }

    #[test]
    #[should_panic(expected = "native-deps.toml")]
    fn row_missing_a_column_panics_naming_the_file() {
        parse_table(&minimal_row_toml("occt").replace("lib_sentinel = \"liboc.so\"\n", ""));
    }

    #[test]
    #[should_panic(expected = "native-deps.toml")]
    fn unknown_column_panics_naming_the_file() {
        parse_table(&format!("{}snapp = 1\n", minimal_row_toml("occt")));
    }

    #[test]
    #[should_panic(expected = "native-deps.toml")]
    fn missing_row_panics_naming_the_file() {
        lookup(&Table::new(), NativeDep::Occt.row_key());
    }

    #[test]
    fn minimal_row_toml_is_itself_valid() {
        let table = parse_table(&minimal_row_toml("occt"));
        assert_eq!(table["occt"].lib_sentinel, "liboc.so");
    }

    /// The real occt row, with candidates that cannot resolve on any host, so
    /// a system OCCT cannot short-circuit the snap scan, and its snap root moved
    /// to `snap_root`. Lays out `<snap_root>/4242/<subdir>/<sentinel>` for both
    /// kinds.
    fn occt_row_with_fixture_snap(snap_root: &Path) -> DepRow {
        let mut row = NativeDep::Occt.row().clone();
        row.include_candidates = vec!["/definitely/does/not/exist/include".to_string()];
        row.lib_candidates = vec!["/definitely/does/not/exist/lib".to_string()];
        row.snap
            .as_mut()
            .expect("the occt row carries a snap table")
            .root = snap_root.to_string_lossy().into_owned();
        let snap = row.snap.as_ref().unwrap();
        for kind in [DirKind::Include, DirKind::Lib] {
            let sentinel = snap_root
                .join("4242")
                .join(snap.subdir(kind))
                .join(row.sentinel(kind));
            fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
            fs::write(&sentinel, b"").unwrap();
        }
        row
    }

    #[test]
    fn snap_fallback_resolves_fixture_root_for_occt() {
        let guard = tempdir();
        let root = guard.path();
        let row = occt_row_with_fixture_snap(root);
        let snap = row.snap.as_ref().unwrap();

        for kind in [DirKind::Include, DirKind::Lib] {
            assert_eq!(
                resolve_dir(None, &row, kind),
                Some(root.join("4242").join(snap.subdir(kind)))
            );
        }
    }

    #[test]
    fn row_without_snap_does_not_scan() {
        let guard = tempdir();
        let mut row = occt_row_with_fixture_snap(guard.path());
        row.snap = None;

        for kind in [DirKind::Include, DirKind::Lib] {
            assert_eq!(resolve_dir(None, &row, kind), None);
        }
    }

    /// A `[<key>]` row with every required column and no optional one.
    fn minimal_row_toml(key: &str) -> String {
        format!(
            "[{key}]\n\
             include_env = \"REIFY_BUILD_UTILS_TEST_UNSET_INCLUDE\"\n\
             lib_env = \"REIFY_BUILD_UTILS_TEST_UNSET_VAR\"\n\
             cfg = \"has_{key}\"\n\
             include_candidates = []\n\
             lib_candidates = []\n\
             include_sentinel = \"oc.h\"\n\
             lib_sentinel = \"liboc.so\"\n"
        )
    }

    /// A row whose lib half probes `lib_candidates` for `libgmsh.so`, with its
    /// override env var named so that nothing sets it.
    fn fixture_row(lib_candidates: &[&str]) -> DepRow {
        let mut row = parse_table(&minimal_row_toml("fixture"))
            .remove("fixture")
            .unwrap();
        row.lib_candidates = lib_candidates.iter().map(|c| c.to_string()).collect();
        row.lib_sentinel = "libgmsh.so".to_string();
        row
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
        let row = fixture_row(&[candidate_str.as_str()]);
        let found = resolve_dir(Some(override_str.as_str()), &row, DirKind::Lib);

        assert_eq!(found.as_deref(), Some(override_dir));
    }

    /// An exported-but-EMPTY override counts as unset. Taken on trust, it
    /// would resolve to the empty path and set `has_<dep>` over a link
    /// against nothing.
    #[test]
    fn find_dir_ignores_exported_but_empty_override() {
        let guard = tempdir();
        let tmp = guard.path();
        fs::write(tmp.join("libgmsh.so"), b"").unwrap();

        let tmp_str = tmp.to_string_lossy().into_owned();
        let row = fixture_row(&[tmp_str.as_str()]);
        let found = resolve_dir(Some(""), &row, DirKind::Lib);

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
        let row = fixture_row(&["/definitely/does/not/exist", tmp_str.as_str()]);
        let found = find_dir(&row, DirKind::Lib);
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
