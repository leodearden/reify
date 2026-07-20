// Engine-version-hash algorithm: single source of truth shared between the
// library crate and `build.rs`.
//
// # Dual-compilation architecture
//
// This file is declared as `pub(crate) mod engine_hash_algo;` in `lib.rs` for
// library use, AND included verbatim into `build.rs` via:
//
//   include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/engine_hash_algo.rs"));
//
// There is ONE copy of the framing + hash + walk algorithm; any change here
// affects both callers simultaneously. This directly addresses reviewer
// issue #2 (algorithm-drift pin): previously `build.rs` had a duplicated
// `push_framed + xxh3_128` implementation that was only loosely pinned by a
// fixed-hex-literal test; now both binaries compile exactly the same source.
//
// # Design constraints
//
// - Uses only `std::path`, `std::fs`, and `xxhash_rust::xxh3::xxh3_128`.
//   No other deps — adding deps would pull them into the build-script compile
//   graph and may conflict with the library's dep tree.
// - No `use reify_types::...` (reify-types is not a build-dep of reify-eval).
// - The `xxh3_128` output formatted as `{:032x}` is byte-identical to
//   `ContentHash::Display` — see `crates/reify-types/src/hash.rs:55-58` —
//   so all existing pinned-hex-literal tests continue to pass.
// - Inner doc comments (`//!`) are intentionally avoided so this file can be
//   `include!()`d into `build.rs` without triggering E0753.
//
// # PRD reference
//
// `docs/prds/v0_3/persistent-fea-cache.md` §"Cache invalidation on engine
// version".

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use xxhash_rust::xxh3::xxh3_128;

/// Contributor paths relative to `CARGO_MANIFEST_DIR` (i.e. `crates/reify-eval/`).
/// Each entry is either a single file or a directory to walk recursively.
/// Directories are walked by [`walk_contributor`] (entries sorted by file name
/// for byte-determinism across platforms).
///
/// # Categories
///
/// 1. FEA solver implementation   — crates/reify-solver-elastic (src/ + Cargo.toml)
/// 2. Meshing pipeline            — crates/reify-kernel-gmsh (src/ + Cargo.toml + build.rs)
/// 3. Stdlib FEA helpers          — crates/reify-stdlib/src/{fea,loads,supports,analysis}.rs
/// 4. Per-purpose tolerance impl  — crates/reify-eval/src/tolerance_*.rs,
///    engine_tolerance.rs, engine_purposes.rs
/// 5. Transitive-dep version pin  — NOT in this list. Narrowed to reify-eval's
///    closure pins and handled by `build.rs` directly (see below).
///
/// # Transitive-dep version pin (formerly category 5 — narrowed, task 5272)
///
/// A transitive dependency version bump — e.g. `nalgebra`, `faer`, or
/// `nalgebra-sparse` — can silently change FEA semantics (different LU pivoting
/// strategy, different eigensolver tolerances) without altering any source byte
/// in the crates listed in categories 1-4.  The persistent FEA cache would then
/// serve stale results across such bumps indefinitely.
///
/// This used to be captured by listing the WHOLE workspace `Cargo.lock` in this
/// array (walked byte-for-byte), which over-invalidated the cache on ANY dep
/// bump anywhere in the ~716-package lockfile — including GUI-only deps like
/// `tauri` that never affect FEA. That contribution is now NARROWED: `build.rs`
/// hashes only the resolved `(name, version)` pins of reify-eval's build+normal
/// (dev-EXCLUDED) transitive closure — the crate NAMES checked in at
/// `crates/reify-eval/engine_hash_closure.txt` — via [`parse_closure_manifest`]
/// and [`cargo_lock_closure_parts`].  So `../../Cargo.lock` is deliberately NOT in
/// `CONTRIBUTORS_RELATIVE`; `build.rs` reads it directly alongside the manifest.
/// The drift guard `tests/infra/test_engine_hash_closure.sh` keeps the manifest
/// a superset (⊇) of the freshly-recomputed closure.
///
/// This still prefers over-invalidation to under-invalidation (a cache miss +
/// recompute vs. silently incorrect FEA results) — but only within reify-eval's
/// own dependency closure, not the whole workspace.  PRDs:
/// `docs/prds/merge-gate-compile-cost.md` §3 W4 / §5 C4;
/// `docs/prds/v0_3/persistent-fea-cache.md` §"Cache invalidation on engine
/// version".
// The non-test library build never references CONTRIBUTORS_RELATIVE — only
// the build.rs binary (via `include!()`) and `#[cfg(test)]` code in
// `persistent_cache.rs` do.  Without the attribute, `cargo build` of the
// library would emit a dead_code warning, mirroring the existing
// `#[allow(dead_code)]` on `ContributorWalk` and `walk_contributor`.
#[allow(dead_code)]
pub(crate) const CONTRIBUTORS_RELATIVE: &[&str] = &[
    // 1. FEA solver
    "../reify-solver-elastic/src",
    "../reify-solver-elastic/Cargo.toml",
    // 2. Meshing pipeline
    "../reify-kernel-gmsh/src",
    "../reify-kernel-gmsh/Cargo.toml",
    "../reify-kernel-gmsh/build.rs",
    // 3. Stdlib FEA helpers
    "../reify-stdlib/src/fea.rs",
    "../reify-stdlib/src/loads.rs",
    "../reify-stdlib/src/supports.rs",
    "../reify-stdlib/src/analysis.rs",
    // 4. Per-purpose tolerance implementation
    "src/tolerance_bucket.rs",
    "src/tolerance_budget.rs",
    "src/tolerance_combine.rs",
    "src/tolerance_format.rs",
    "src/tolerance_gate.rs",
    "src/tolerance_promise.rs",
    "src/tolerance_scope.rs",
    "src/engine_tolerance.rs",
    "src/engine_purposes.rs",
    // 5. Transitive-dep version pin — NOT here (task 5272). Narrowed to
    //    reify-eval's build+normal (dev-excluded) closure pins: build.rs reads
    //    ../../Cargo.lock + engine_hash_closure.txt directly and hashes only the
    //    matching (name, version) pins via cargo_lock_closure_parts. See the
    //    CONTRIBUTORS_RELATIVE doc-comment above for the full rationale.
];

/// Suffix set shared by the bare dot-prefix branch and the extension branch of
/// [`is_editor_debris`].  Kept as a single constant so both branches always
/// test the same set — a future addition to one cannot silently diverge from
/// the other.
#[allow(dead_code)]
const DEBRIS_SUFFIXES: &[&str] = &["swp", "swo", "swn", "bk", "bak", "orig", "rej", "tmp"];

/// Returns true when `file_name` matches a known editor or OS debris pattern.
///
/// Applied during directory iteration (after sorting, before recursion) so
/// transient editor artifacts never enter the hash input or the
/// `cargo:rerun-if-changed` directive list.  Explicit single-file contributors
/// listed in `build.rs` are **not** passed through this filter (filtering only
/// happens inside the directory-enumeration branch of `walk_recursive`).
///
/// **Root-directory bypass:** When `walk_recursive` enters a directory, the
/// directory's own name is never tested against this filter — only the names
/// of its children are.  A caller that names a top-level contributor whose
/// final path component matches a debris pattern (e.g. `/tmp/.DS_Store/`)
/// will still have the directory entered and its contents walked.  This is
/// intentional: the filter's purpose is to exclude transient files that
/// appear *alongside* real sources, not to gate which top-level contributors
/// the caller may name.
///
/// # Denylist rationale
///
/// This is a denylist, not an allowlist.  An allowlist would silently exclude
/// any new legitimate contributor file extension (`.rs`, `.toml`, `.py`, etc.)
/// the moment it appears in a contributor directory — the exact failure mode
/// that motivated adding recursive directory walking in the first place.  A
/// denylist explicitly names known-transient artifacts and lets everything else
/// through; the cost of missing an obscure pattern is a one-off hash divergence
/// that is easy to diagnose.
///
/// # Patterns matched
///
/// | Pattern | Examples |
/// |---------|---------|
/// | Extension in `{swp, swo, swn, bk, bak, orig, rej, tmp}` | `.foo.swp`, `bar.orig` |
/// | Bare dot-prefixed name in `{swp, swo, swn, bk, bak, orig, rej, tmp}` | `.swp`, `.bak` |
/// | Exact name (case-insensitive) `{.ds_store, thumbs.db, desktop.ini}` | `.DS_Store` |
/// | Name ending with `~` | `foo.rs~` (Emacs backup) |
///
/// All four rules apply uniformly to both files and directory names encountered
/// during enumeration.
// Used by `walk_recursive` which is itself `#[allow(dead_code)]`.
// See walk_contributor for inline-never workaround history (task 3429).
#[allow(dead_code)]
fn is_editor_debris(file_name: &OsStr) -> bool {
    let name_lower = file_name.to_string_lossy().to_lowercase();

    // Emacs backup: file ends with `~`.
    if name_lower.ends_with('~') {
        return true;
    }

    // Exact-name matches (case-insensitive).
    if matches!(
        name_lower.as_str(),
        ".ds_store" | "thumbs.db" | "desktop.ini"
    ) {
        return true;
    }

    // Bare dot-prefixed name with no stem (e.g. `.swp`, `.bak`).
    // `Path::extension()` returns `None` for these because the leading dot is
    // treated as the beginning of the stem, not as a separator — so the
    // extension branch below would silently miss them.  Strip the leading dot
    // and match the remainder against DEBRIS_SUFFIXES.
    if let Some(stripped) = name_lower.strip_prefix('.')
        && DEBRIS_SUFFIXES.contains(&stripped)
    {
        return true;
    }

    // Extension-based matches: extract the last `.`-delimited component.
    if let Some(ext) = std::path::Path::new(file_name).extension() {
        let ext_lower = ext.to_string_lossy().to_lowercase();
        if DEBRIS_SUFFIXES.contains(&ext_lower.as_str()) {
            return true;
        }
    }

    false
}

/// Compute the canonical engine-version hash for a set of contributor byte slices.
///
/// Each contributor is framed with a `u64` LE length prefix before concatenation
/// into the hash buffer. This prevents the trivial concat-collision class where
/// `[b"ab", b"c"]` and `[b"a", b"bc"]` would otherwise produce identical hashes
/// (see `compose_engine_version_hash_length_prefix_prevents_concat_collision`).
///
/// The hash primitive is `xxhash_rust::xxh3::xxh3_128` — the same algorithm
/// used by `reify_types::ContentHash`, formatted identically (`{:032x}` matches
/// `ContentHash::Display` from `crates/reify-types/src/hash.rs:55-58`).
/// Cache-key invalidation does not require cryptographic collision resistance;
/// xxh3 is appropriate and consistent with existing conventions.
///
/// Returns a 32-character lowercase hexadecimal string.
///
/// **Production caller**: `build.rs` calls this after accumulating all
/// contributor walk parts (via [`walk_contributor`]). The function is `pub`
/// so `persistent_cache::ENGINE_VERSION_HASH`'s doc comment can reference it
/// by name, and so the algorithm-drift sentinel test
/// (`compose_engine_version_hash_pins_fixed_input_to_exact_hex_literal`)
/// pins the single canonical algorithm shared by both the library and the
/// build script.
///
/// PRD: `docs/prds/v0_3/persistent-fea-cache.md` §"Cache invalidation on engine
/// version".
pub fn compose_engine_version_hash(parts: &[&[u8]]) -> String {
    let total_len: usize = parts.iter().map(|p| 8 + p.len()).sum();
    let mut buf = Vec::with_capacity(total_len);
    for part in parts {
        buf.extend_from_slice(&(part.len() as u64).to_le_bytes());
        buf.extend_from_slice(part);
    }
    let h = xxh3_128(&buf);
    format!("{:032x}", h)
}

/// Result of walking a contributor file or directory tree.
///
/// Returned by [`walk_contributor`]. Fields are populated in sorted
/// (deterministic) order, ready for direct use by `build.rs` and the
/// equivalence tests.
// Used by `build.rs` (via `include!()`) and by `#[cfg(test)]` blocks in
// `persistent_cache.rs`. Neither site is visible to the non-test lib
// compiler, so we suppress the dead_code lint here.
#[allow(dead_code)]
pub struct ContributorWalk {
    /// Interleaved `(path_bytes, file_bytes)` pairs, each stored as a `Vec<u8>`.
    ///
    /// To pass to [`compose_engine_version_hash`], convert to `Vec<&[u8]>`:
    /// ```ignore
    /// let refs: Vec<&[u8]> = walk.parts.iter().map(|v| v.as_slice()).collect();
    /// let hash = compose_engine_version_hash(&refs);
    /// ```
    pub parts: Vec<Vec<u8>>,
    /// Paths to emit as `cargo:rerun-if-changed` directives.
    ///
    /// Includes BOTH **file paths** AND **directory paths** (the root and
    /// every sub-directory visited). Directory-level entries are the
    /// issue-#1 fix: cargo only re-runs a build script when at least one
    /// listed path changes; with file-only entries, adding a brand-new source
    /// file to a contributor directory silently fails to trigger a rebuild and
    /// the new file's bytes are absent from `ENGINE_VERSION_HASH`. Emitting
    /// the containing directory causes cargo to re-run when the directory's
    /// child set changes (file added / renamed / removed), closing the gap.
    pub rerun_paths: Vec<PathBuf>,
}

/// Walk a contributor file or directory tree, collecting
/// `(path_bytes, file_bytes)` pairs and rerun-if-changed paths.
///
/// # Single-file root
///
/// When `root` is a regular file, `path_bytes = label.as_bytes()` and
/// `file_bytes = fs::read(root)`. The rerun list contains only `root`.
///
/// # Directory root
///
/// When `root` is a directory, the walk is recursive. Entries are sorted by
/// file name for byte-determinism across platforms (filesystem iteration order
/// is unspecified and varies between ext4, APFS, NTFS, etc.).
///
/// `path_bytes` for each file is `"{label}/{relative_path}"` where
/// `relative_path` is the file's path relative to `root`. Including the path
/// in the hash means renaming a file changes the hash even when content is
/// identical — the desired semantics (contributor identity matters, not just
/// bytes).
///
/// The rerun list includes `root`, every sub-directory, and every file, so
/// adding or removing a file in a contributor directory triggers a rebuild
/// even though the new file was not previously listed.
///
/// # Panics
///
/// Panics with an `ENGINE_VERSION_HASH:` prefix on any I/O error. Silent
/// skips would let the cache key drift unnoticed if a contributor source
/// becomes unreadable.
// Used by `build.rs` (via `include!()`) and by `#[cfg(test)]` blocks in
// `persistent_cache.rs`. Neither site is visible to the non-test lib
// compiler, so we suppress the dead_code lint here.
//
// A previous `#[inline(never)]` workaround on `walk_contributor`,
// `walk_recursive`, and `is_editor_debris` was removed in task 3429 (original
// workaround added in commit 95b3d3c6af). Re-verification on 2026-05-11
// confirmed the attributes are no longer needed:
//   rustc 1.94.1 (e408947bf 2026-03-25), LLVM 21.1.8
//   narrow repro: cargo test --release -p reify-eval --test kinematic_sweep_closed_chain
//   full suite:   cargo test --release -p reify-eval (2116 tests, all passed)
//   base commit: 65a7156bd40ee9d47c400f6f50a4d6a52212130e
// The symmetric attributes on `walk_recursive` and `is_editor_debris` were removed in the
// same commit.
#[allow(dead_code)]
pub fn walk_contributor(label: &str, root: &Path) -> ContributorWalk {
    let mut walk = ContributorWalk {
        parts: Vec::new(),
        rerun_paths: Vec::new(),
    };
    walk_recursive(label, root, root, &mut walk);
    walk
}

// Called only from `walk_contributor` which is itself `#[allow(dead_code)]`;
// suppress the lint here too so the compiler doesn't complain about the
// transitively unreachable private function in the non-test lib build.
// See walk_contributor for inline-never workaround history (task 3429).
#[allow(dead_code)]
fn walk_recursive(label: &str, root: &Path, path: &Path, walk: &mut ContributorWalk) {
    // Use symlink_metadata so we dispatch on the type of `path` itself, NOT
    // the type of whatever `path` points to through symlink chains.
    // Path::is_file() / Path::is_dir() call fs::metadata(), which follows
    // symlinks — so a symlink to a regular file passes is_file() and would be
    // walked, making the hash machine-specific.  symlink_metadata() does not
    // follow links, so symlinks are typed as symlinks and fall through to the
    // silent-skip at the end of the if-let block.
    // Silently skip entries where symlink_metadata() fails (broken
    // symlinks, races where an entry is removed between read_dir and the
    // type check, transient permission issues).  This preserves today's
    // behavior where Path::is_file/is_dir already returned false on
    // metadata errors — no new panic paths.
    if let Ok(meta) = path.symlink_metadata() {
        let ft = meta.file_type();
        if ft.is_file() {
            walk.rerun_paths.push(path.to_path_buf());
            let path_bytes: Vec<u8> = if path == root {
                // Single-file root: use the label as the path key.
                label.as_bytes().to_vec()
            } else {
                // File within a directory: use "{label}/{relative_path}".
                let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy();
                format!("{label}/{rel}").into_bytes()
            };
            let file_bytes = std::fs::read(path).unwrap_or_else(|e| {
                panic!(
                    "ENGINE_VERSION_HASH: cannot read contributor {}: {e}",
                    path.display()
                )
            });
            walk.parts.push(path_bytes);
            walk.parts.push(file_bytes);
        } else if ft.is_dir() {
            // Emit the directory itself so cargo re-runs when files are
            // added or removed — not only when an already-listed file
            // changes (issue #1 fix).
            walk.rerun_paths.push(path.to_path_buf());
            let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
                .unwrap_or_else(|e| {
                    panic!(
                        "ENGINE_VERSION_HASH: cannot read dir {}: {e}",
                        path.display()
                    )
                })
                .map(|e| {
                    e.unwrap_or_else(|e| {
                        panic!(
                            "ENGINE_VERSION_HASH: dir entry error in {}: {e}",
                            path.display()
                        )
                    })
                    .path()
                })
                .collect();
            // Sort for byte-determinism across platforms.
            entries.sort_by(|a, b| {
                a.file_name()
                    .unwrap_or_default()
                    .cmp(b.file_name().unwrap_or_default())
            });
            // Drop known editor/OS debris before recursing so transient
            // files never perturb the hash or cargo:rerun-if-changed
            // directives.  `is_none_or` retains entries with no
            // file-name component (impossible for read_dir results, but
            // satisfies Option without unwrap).
            entries.retain(|p| p.file_name().is_none_or(|n| !is_editor_debris(n)));
            for entry in entries {
                walk_recursive(label, root, &entry, walk);
            }
            // Symlinks, FIFOs, sockets, devices, and other non-regular
            // entries fall through here — silently skipped.
        }
    }
    // Only regular files and directories contribute to the hash.
    // symlink_metadata() (used above) does NOT follow symlinks — so symlinks
    // (whether pointing to files or directories), broken symlinks, FIFOs,
    // sockets, character/block devices, and other non-regular entries are
    // silently skipped.  Path::is_file/is_dir would have followed symlinks
    // via fs::metadata; that is why we use symlink_metadata instead.  Cache
    // determinism requires that machine-local symlinks (which may resolve to
    // absolute paths that differ per developer or CI host) never enter the
    // hash input.
}

// ─────────────────────────────────────────────────────────────────────────────
// Narrowed Cargo.lock contribution (task 5272)
//
// ENGINE_VERSION_HASH used to walk the WHOLE workspace Cargo.lock (category 5),
// so any dep bump anywhere in the 716-package lockfile invalidated the
// persistent FEA cache. The helpers below narrow that contribution to only the
// resolved (name, version) pins of reify-eval's build+normal (exclude-dev)
// transitive closure — the crate NAMES checked in at
// `crates/reify-eval/engine_hash_closure.txt`. build.rs reads that static
// manifest plus Cargo.lock and hashes just the matching pins; the drift guard
// `tests/infra/test_engine_hash_closure.sh` keeps the manifest honest against
// the live closure. PRD: docs/prds/merge-gate-compile-cost.md §3 W4 / §5 C4.
//
// These are pure, std-only functions (NO `toml` crate — this file is include!'d
// into build.rs, which is constrained to std + xxhash per the module header).
// Each is `pub` + `#[allow(dead_code)]`, mirroring CONTRIBUTORS_RELATIVE /
// walk_contributor: reachable from build.rs (via include!) and from
// persistent_cache.rs `#[cfg(test)]`, but not from the non-test library build.
// ─────────────────────────────────────────────────────────────────────────────

/// Parse the `[[package]]` stanzas of a Cargo.lock file into `(name, version)`
/// pairs, in file order.
///
/// Hand-rolled, std-only line scanner (deliberately NOT the `toml` crate — see
/// the section header above). State machine: a `[[package]]` header opens a
/// fresh stanza; the first `name = "..."` and first `version = "..."` lines
/// within it are captured; the pair is emitted when the next `[`-prefixed table
/// header is reached (or at EOF). Anything outside a `[[package]]` stanza — the
/// top-level lockfile `version = N`, `[metadata]`, `[[patch.unused]]`, comments,
/// blank lines — never yields a package. Inside a stanza, `source` / `checksum`
/// / multi-line `dependencies = [...]` lines are ignored (only name+version are
/// captured); array elements like `"memchr",` carry no `=` and are skipped.
#[allow(dead_code)]
pub fn parse_cargo_lock_packages(lock_text: &str) -> Vec<(String, String)> {
    // Content of the first `"..."` in `s`, if any (values here never contain an
    // embedded quote, so first-open .. next-close is sufficient and exact).
    fn first_quoted(s: &str) -> Option<String> {
        let start = s.find('"')?;
        let rest = &s[start + 1..];
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    }
    // Emit the pending stanza iff BOTH name and version were captured. `take()`
    // empties both Options regardless of the match, resetting for the next
    // stanza; harmless to call when nothing is pending (both already None).
    fn flush(
        cur_name: &mut Option<String>,
        cur_version: &mut Option<String>,
        packages: &mut Vec<(String, String)>,
    ) {
        if let (Some(n), Some(v)) = (cur_name.take(), cur_version.take()) {
            packages.push((n, v));
        }
    }

    let mut packages: Vec<(String, String)> = Vec::new();
    let mut in_package = false;
    let mut cur_name: Option<String> = None;
    let mut cur_version: Option<String> = None;

    for line in lock_text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            // Any table header closes the current stanza; only `[[package]]`
            // opens a new capturing one.
            flush(&mut cur_name, &mut cur_version, &mut packages);
            in_package = trimmed == "[[package]]";
            continue;
        }
        if !in_package {
            continue;
        }
        // Inside a [[package]] stanza: capture the first name/version. Split on
        // the first `=`; array elements ("memchr",) and bare `]` carry no `=`.
        if let Some(eq) = trimmed.find('=') {
            let key = trimmed[..eq].trim();
            let val = &trimmed[eq + 1..];
            if key == "name" && cur_name.is_none() {
                cur_name = first_quoted(val);
            } else if key == "version" && cur_version.is_none() {
                cur_version = first_quoted(val);
            }
        }
    }
    // EOF: flush a trailing package stanza (no closing header follows it).
    flush(&mut cur_name, &mut cur_version, &mut packages);
    packages
}

/// Parse a closure manifest (`crates/reify-eval/engine_hash_closure.txt`) into
/// its crate names, in file order.
///
/// Skips blank / whitespace-only lines and `#`-comment lines — a line whose
/// first non-whitespace character is `#`, so indented comments count too. Every
/// other line is trimmed and returned as a name.
#[allow(dead_code)]
pub fn parse_closure_manifest(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_string())
        .collect()
}

/// Filter parsed Cargo.lock packages down to reify-eval's closure and return
/// their `(name, version)` pins, sorted canonically by `(name, version)`.
///
/// `closure` is the list of crate NAMES from `engine_hash_closure.txt` (see
/// [`parse_closure_manifest`]). Filtering is by NAME: a closure name absent
/// from the lock contributes nothing, and two same-named stanzas at different
/// versions BOTH survive (the over-approximating, safe-to-over-invalidate
/// direction). Sorting makes the result order-INDEPENDENT of stanza order in
/// the lock; lexicographic tuple order is sufficient (only determinism is
/// required, not semver ordering).
#[allow(dead_code)]
pub fn cargo_lock_closure_pins(lock_text: &str, closure: &[&str]) -> Vec<(String, String)> {
    use std::collections::HashSet;
    let wanted: HashSet<&str> = closure.iter().copied().collect();
    let mut pins: Vec<(String, String)> = parse_cargo_lock_packages(lock_text)
        .into_iter()
        .filter(|(name, _)| wanted.contains(name.as_str()))
        .collect();
    pins.sort();
    pins
}

/// Frame reify-eval's closure pins as hash byte parts: for each pin, in
/// `(name, version)`-sorted order, push the `name` bytes then the `version`
/// bytes as two separate parts.
///
/// Two parts per pin leverages the existing u64-LE length-prefix framing in
/// [`compose_engine_version_hash`], which already prevents the concat-collision
/// class — so no custom separator between the name and version is needed.
/// `build.rs` extends its `all_parts` with the result, replacing the removed
/// whole-file Cargo.lock walk.
#[allow(dead_code)]
pub fn cargo_lock_closure_parts(lock_text: &str, closure: &[&str]) -> Vec<Vec<u8>> {
    let mut parts: Vec<Vec<u8>> = Vec::new();
    for (name, version) in cargo_lock_closure_pins(lock_text, closure) {
        parts.push(name.into_bytes());
        parts.push(version.into_bytes());
    }
    parts
}
