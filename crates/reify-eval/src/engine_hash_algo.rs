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

use std::path::{Path, PathBuf};
use xxhash_rust::xxh3::xxh3_128;

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
pub fn walk_contributor(label: &str, root: &Path) -> ContributorWalk {
    let mut walk = ContributorWalk {
        parts: Vec::new(),
        rerun_paths: Vec::new(),
    };
    walk_recursive(label, root, root, &mut walk);
    walk
}

fn walk_recursive(label: &str, root: &Path, path: &Path, walk: &mut ContributorWalk) {
    if path.is_file() {
        walk.rerun_paths.push(path.to_path_buf());
        let path_bytes: Vec<u8> = if path == root {
            // Single-file root: use the label as the path key.
            label.as_bytes().to_vec()
        } else {
            // File within a directory: use "{label}/{relative_path}".
            let rel = path
                .strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy();
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
    } else if path.is_dir() {
        // Emit the directory itself so cargo re-runs when files are added or
        // removed — not only when an already-listed file changes (issue #1 fix).
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
        for entry in entries {
            walk_recursive(label, root, &entry, walk);
        }
    }
    // Symlinks and other non-file/dir entries are intentionally skipped.
}
