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

/// Every workspace path crate in reify-eval's dependency closure, and whether
/// its sources contribute to `ENGINE_VERSION_HASH`. Rows are sorted by crate
/// name; [`contributor_paths`] flattens the [`Coverage::Hashed`] rows in that
/// order into the hash input.
///
/// # Hashing boundary
///
/// The persistent key already folds the content hash of each evaluated
/// argument `Value` (`engine_eval.rs` `persistent_cache_key`). What it does not
/// see is the code that turns those arguments into the persisted payload: the
/// code that runs inside a persisted target's dispatch (its trampoline and
/// everything that trampoline calls). That code must be hashed here (PRD:
/// "Any change capable of affecting result values must contribute").
///
/// A path crate's Cargo.lock version is a constant `0.1.0`, so the closure pin
/// below never invalidates on its source change: each one is therefore either
/// [`Coverage::Hashed`] or excluded for a [`NotHashedBecause`] reason.
/// `src/engine_hash_tests.rs` enforces a row per path crate, and that a byte
/// change in each file of its hand-kept `PERSISTED_TARGET_SOURCES` table moves
/// the hash. A partially hashed crate (reify-eval, reify-stdlib) lists the
/// files it hashes, so its soundness rests on that table naming every file of
/// the crate a persisted dispatch runs; an unlisted callee goes undetected.
///
/// reify-core and reify-ir are hashed whole although the trampolines reach
/// only part of them (Value/Diagnostic, arg_acceptance, the sampled-field
/// helpers), so almost any edit to either flushes the persistent FEA cache.
/// That trades hit rate for soundness, per the PRD's prefer-over-invalidation
/// policy; a per-file narrowing would inherit the partial-crate caveat above.
///
/// # Transitive-dep version pin (narrowed, task 5272)
///
/// A transitive dependency version bump — e.g. `nalgebra`, `faer`, or
/// `nalgebra-sparse` — can silently change FEA semantics (different LU pivoting
/// strategy, different eigensolver tolerances) without altering any source byte
/// listed here.  The persistent FEA cache would then serve stale results across
/// such bumps indefinitely.
///
/// This used to be captured by hashing the WHOLE workspace `Cargo.lock`
/// byte-for-byte, which over-invalidated the cache on ANY dep bump anywhere in
/// the ~716-package lockfile — including GUI-only deps like `tauri` that never
/// affect FEA. That contribution is now NARROWED: [`engine_version_hash_for`]
/// hashes only the resolved `(name, version)` pins of reify-eval's
/// build+normal (dev-EXCLUDED) transitive closure — the crate NAMES checked in
/// at `crates/reify-eval/engine_hash_closure.txt` — via
/// [`parse_closure_manifest`] and [`cargo_lock_closure_parts`].  So
/// `../../Cargo.lock` is deliberately NOT a contributor path; it is read
/// directly alongside the manifest. The drift guard
/// `tests/infra/test_engine_hash_closure.sh` keeps the manifest a superset (⊇)
/// of the freshly-recomputed closure.
///
/// This still prefers over-invalidation to under-invalidation (a cache miss +
/// recompute vs. silently incorrect FEA results) — but only within reify-eval's
/// own dependency closure, not the whole workspace.  PRDs:
/// `docs/prds/merge-gate-compile-cost.md` §3 W4 / §5 C4;
/// `docs/prds/v0_3/persistent-fea-cache.md` §"Cache invalidation on engine
/// version".
///
/// # Soundness of the closure pin
///
/// The pin keys ONLY on each package's resolved `(name, version)`:
/// - a **registry** crate (`source = "registry+…"`): the version fully
///   determines the content, so the pin is exact;
/// - a **path** crate (no `source` line): the version is a constant, so the pin
///   gives no invalidation, and its source must be hashed in this table or
///   excluded for a stated reason;
/// - a **git** or `[patch]` source: the content can change without a version
///   bump, so the pin would be under-tight. None is allowed in the closure —
///   `every_non_path_closure_member_is_registry_sourced` rejects one — until
///   [`cargo_lock_closure_pins`] folds that stanza's `source`/`checksum` in.
// build.rs (via include!) reads crate names and Hashed paths but never a
// NotHashedBecause payload; the non-test lib build reaches none of these items.
#[allow(dead_code)]
pub(crate) const WORKSPACE_CRATE_COVERAGE: &[WorkspaceCrateCoverage] = &[
    WorkspaceCrateCoverage {
        crate_name: "reify-ast",
        coverage: Coverage::NotHashed(NotHashedBecause::UpstreamOfPersistentKey),
    },
    WorkspaceCrateCoverage {
        crate_name: "reify-build-utils",
        coverage: Coverage::NotHashed(NotHashedBecause::NotOnAPersistedDispatchPath),
    },
    WorkspaceCrateCoverage {
        crate_name: "reify-builtins",
        coverage: Coverage::NotHashed(NotHashedBecause::UpstreamOfPersistentKey),
    },
    WorkspaceCrateCoverage {
        crate_name: "reify-compiler",
        coverage: Coverage::NotHashed(NotHashedBecause::UpstreamOfPersistentKey),
    },
    // The persisted ElasticResult contract types.
    WorkspaceCrateCoverage {
        crate_name: "reify-compute-contract",
        coverage: Coverage::Hashed(&[
            "../reify-compute-contract/src",
            "../reify-compute-contract/Cargo.toml",
        ]),
    },
    WorkspaceCrateCoverage {
        crate_name: "reify-config",
        coverage: Coverage::NotHashed(NotHashedBecause::NotOnAPersistedDispatchPath),
    },
    WorkspaceCrateCoverage {
        crate_name: "reify-constraints",
        coverage: Coverage::NotHashed(NotHashedBecause::UpstreamOfPersistentKey),
    },
    // Value / Diagnostic construction in every trampoline.
    WorkspaceCrateCoverage {
        crate_name: "reify-core",
        coverage: Coverage::Hashed(&["../reify-core/src", "../reify-core/Cargo.toml"]),
    },
    // The persisted targets' trampolines (compute_targets, shell_extract_compute),
    // their BC face resolution (topology_selectors + selector_vocabulary_v2) and
    // the per-purpose tolerance code. Other reify-eval code evaluates the
    // arguments (upstream of the key) or runs in the dispatch without shaping
    // values: solver_progress (progress/cancellation), compute_cache_key (key
    // only), persistent_cache (wire format, versioned by ENTRY_FORMAT_VERSION).
    // Re-exported contract types live in reify-ir and reify-compute-contract.
    WorkspaceCrateCoverage {
        crate_name: "reify-eval",
        coverage: Coverage::Hashed(&[
            "src/compute_targets",
            "src/engine_purposes.rs",
            "src/engine_tolerance.rs",
            "src/selector_vocabulary_v2.rs",
            "src/shell_extract_compute.rs",
            "src/tolerance_bucket.rs",
            "src/tolerance_budget.rs",
            "src/tolerance_combine.rs",
            "src/tolerance_format.rs",
            "src/tolerance_gate.rs",
            "src/tolerance_promise.rs",
            "src/tolerance_scope.rs",
            "src/topology_selectors.rs",
        ]),
    },
    WorkspaceCrateCoverage {
        crate_name: "reify-expr",
        coverage: Coverage::NotHashed(NotHashedBecause::UpstreamOfPersistentKey),
    },
    // As-printed zone classification (classify_point) inside elastic_static.
    WorkspaceCrateCoverage {
        crate_name: "reify-fdm",
        coverage: Coverage::Hashed(&["../reify-fdm/src", "../reify-fdm/Cargo.toml"]),
    },
    WorkspaceCrateCoverage {
        crate_name: "reify-gcode",
        coverage: Coverage::NotHashed(NotHashedBecause::NotOnAPersistedDispatchPath),
    },
    // arg_acceptance and the sampled-field helpers run in every trampoline.
    WorkspaceCrateCoverage {
        crate_name: "reify-ir",
        coverage: Coverage::Hashed(&["../reify-ir/src", "../reify-ir/Cargo.toml"]),
    },
    // The mesher reify-solver-elastic calls inside the dispatch.
    WorkspaceCrateCoverage {
        crate_name: "reify-kernel-gmsh",
        coverage: Coverage::Hashed(&[
            "../reify-kernel-gmsh/src",
            "../reify-kernel-gmsh/Cargo.toml",
            "../reify-kernel-gmsh/build.rs",
        ]),
    },
    WorkspaceCrateCoverage {
        crate_name: "reify-kernel-openvdb",
        coverage: Coverage::NotHashed(NotHashedBecause::RealizationProducer),
    },
    WorkspaceCrateCoverage {
        crate_name: "reify-shell-extract",
        coverage: Coverage::Hashed(&[
            "../reify-shell-extract/src",
            "../reify-shell-extract/Cargo.toml",
        ]),
    },
    WorkspaceCrateCoverage {
        crate_name: "reify-solver-elastic",
        coverage: Coverage::Hashed(&[
            "../reify-solver-elastic/src",
            "../reify-solver-elastic/Cargo.toml",
        ]),
    },
    // Only the FEA helpers; the rest of stdlib evaluates the arguments, which
    // is upstream of the persistent key.
    WorkspaceCrateCoverage {
        crate_name: "reify-stdlib",
        coverage: Coverage::Hashed(&[
            "../reify-stdlib/src/analysis.rs",
            "../reify-stdlib/src/fea.rs",
            "../reify-stdlib/src/loads.rs",
            "../reify-stdlib/src/supports.rs",
        ]),
    },
    WorkspaceCrateCoverage {
        crate_name: "reify-syntax",
        coverage: Coverage::NotHashed(NotHashedBecause::UpstreamOfPersistentKey),
    },
    WorkspaceCrateCoverage {
        crate_name: "tree-sitter-reify",
        coverage: Coverage::NotHashed(NotHashedBecause::UpstreamOfPersistentKey),
    },
];

/// One row of [`WORKSPACE_CRATE_COVERAGE`]: a workspace path crate, named as in
/// Cargo.lock.
#[allow(dead_code)]
pub(crate) struct WorkspaceCrateCoverage {
    pub crate_name: &'static str,
    pub coverage: Coverage,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub(crate) enum Coverage {
    /// Contributor paths relative to `crates/reify-eval`, each inside the row's
    /// own crate: a file, or a directory walked recursively by
    /// [`walk_contributor`].
    Hashed(&'static [&'static str]),
    NotHashed(NotHashedBecause),
}

/// Why a workspace path crate's sources can soundly stay out of the hash.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub(crate) enum NotHashedBecause {
    /// The crate only shapes a persisted target's evaluated argument `Value`s,
    /// and `engine_eval.rs` `persistent_cache_key` folds their content hashes
    /// into the persistent key, so a change re-keys rather than serving stale.
    UpstreamOfPersistentKey,
    /// The crate's output reaches a trampoline only through a
    /// `RealizationReadHandle`, and none of its code runs inside the dispatch.
    /// Realization inputs are recipe-keyed (`graph.rs`
    /// `RealizationNodeData.content_hash`), so soundness for producer output
    /// belongs to the persistent key, not to this hash.
    RealizationProducer,
    /// None of the crate's code runs inside a persisted dispatch or shapes its
    /// arguments (build-time helpers, G-code parsing, `reify.toml` parsing).
    NotOnAPersistedDispatchPath,
}

/// The [`Coverage::Hashed`] paths of [`WORKSPACE_CRATE_COVERAGE`], in table
/// order: the contributor half of the `ENGINE_VERSION_HASH` input.
#[allow(dead_code)]
pub(crate) fn contributor_paths() -> impl Iterator<Item = &'static str> {
    WORKSPACE_CRATE_COVERAGE
        .iter()
        .filter_map(|row| match row.coverage {
            Coverage::Hashed(paths) => Some(paths),
            Coverage::NotHashed(_) => None,
        })
        .flatten()
        .copied()
}

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
/// **Production caller**: [`engine_version_hash_for`] calls this after
/// accumulating all contributor walk parts (via [`walk_contributor`]). The function is `pub`
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
//   narrow repro: cargo test --release -p reify-eval --test harness_fea_solver_e2e -- kinematic_sweep_closed_chain::
//   full suite:   cargo test --release -p reify-eval (2116 tests, all passed)
//   base commit: 65a7156bd40ee9d47c400f6f50a4d6a52212130e
// The symmetric attributes on `walk_recursive` and `is_editor_debris` were removed in the
// same commit.
#[allow(dead_code)]
// G-allow: build-script code — reached via engine_version_hash_for from crates/reify-eval/build.rs (include!, outside this audit's src scope); the lib's own callers are unit tests (see comment above)
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
// ENGINE_VERSION_HASH used to walk the WHOLE workspace Cargo.lock, so any dep
// bump anywhere in the 716-package lockfile invalidated the persistent FEA
// cache. The helpers below narrow that contribution to only the
// resolved (name, version) pins of reify-eval's build+normal (exclude-dev)
// transitive closure — the crate NAMES checked in at
// `crates/reify-eval/engine_hash_closure.txt`. engine_version_hash_for reads
// that static manifest plus Cargo.lock and hashes just the matching pins; the
// drift guard `tests/infra/test_engine_hash_closure.sh` keeps the manifest
// honest against the live closure. PRD: docs/prds/merge-gate-compile-cost.md
// §3 W4 / §5 C4.
//
// These are pure, std-only functions (NO `toml` crate — this file is include!'d
// into build.rs, which is constrained to std + xxhash per the module header).
// Each is `pub` + `#[allow(dead_code)]`, mirroring WORKSPACE_CRATE_COVERAGE /
// walk_contributor: reachable from build.rs (via include!) and from
// persistent_cache.rs `#[cfg(test)]`, but not from the non-test library build.
// ─────────────────────────────────────────────────────────────────────────────

/// One `[[package]]` stanza of a Cargo.lock file.
///
/// `source` is the stanza's `source = "..."` value — `registry+…` for a
/// crates.io dependency, `git+…` for a git dependency — and `None` for a
/// workspace path crate, which Cargo.lock records without a source line.
// build.rs (via include!) never reads `source`, and the non-test lib build
// never constructs a LockPackage at all.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockPackage {
    pub name: String,
    pub version: String,
    pub source: Option<String>,
}

/// Parse the `[[package]]` stanzas of a Cargo.lock file, in file order.
///
/// Hand-rolled, std-only line scanner (deliberately NOT the `toml` crate — see
/// the section header above). State machine: a `[[package]]` header opens a
/// fresh stanza; the first `name = "..."`, first `version = "..."` and first
/// `source = "..."` lines within it are captured; the stanza is emitted when
/// the next `[`-prefixed table header is reached (or at EOF), and only if both
/// a name and a version were captured. Anything outside a `[[package]]` stanza
/// — the top-level lockfile `version = N`, `[metadata]`, `[[patch.unused]]`,
/// comments, blank lines — never yields a package. Inside a stanza,
/// `checksum` / multi-line `dependencies = [...]` lines are ignored; array
/// elements like `"memchr",` carry no `=` and are skipped.
#[allow(dead_code)]
pub fn parse_cargo_lock_stanzas(lock_text: &str) -> Vec<LockPackage> {
    // Content of the first `"..."` in `s`, if any (values here never contain an
    // embedded quote, so first-open .. next-close is sufficient and exact).
    fn first_quoted(s: &str) -> Option<String> {
        let start = s.find('"')?;
        let rest = &s[start + 1..];
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    }
    #[derive(Default)]
    struct PendingStanza {
        name: Option<String>,
        version: Option<String>,
        source: Option<String>,
    }
    // Emit the pending stanza iff BOTH name and version were captured. `take()`
    // resets the pending stanza regardless of the match; harmless to call when
    // nothing is pending.
    fn flush(pending: &mut PendingStanza, packages: &mut Vec<LockPackage>) {
        let PendingStanza {
            name,
            version,
            source,
        } = std::mem::take(pending);
        if let (Some(name), Some(version)) = (name, version) {
            packages.push(LockPackage {
                name,
                version,
                source,
            });
        }
    }

    let mut packages: Vec<LockPackage> = Vec::new();
    let mut in_package = false;
    let mut pending = PendingStanza::default();

    for line in lock_text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            // Any table header closes the current stanza; only `[[package]]`
            // opens a new capturing one.
            flush(&mut pending, &mut packages);
            in_package = trimmed == "[[package]]";
            continue;
        }
        if !in_package {
            continue;
        }
        // Inside a [[package]] stanza: capture the first name/version/source.
        // Split on the first `=`; array elements ("memchr",) and bare `]`
        // carry no `=`.
        if let Some(eq) = trimmed.find('=') {
            let key = trimmed[..eq].trim();
            let val = &trimmed[eq + 1..];
            let slot = match key {
                "name" => &mut pending.name,
                "version" => &mut pending.version,
                "source" => &mut pending.source,
                _ => continue,
            };
            if slot.is_none() {
                *slot = first_quoted(val);
            }
        }
    }
    // EOF: flush a trailing package stanza (no closing header follows it).
    flush(&mut pending, &mut packages);
    packages
}

/// The `(name, version)` projection of [`parse_cargo_lock_stanzas`], in file
/// order.
#[allow(dead_code)]
// G-allow: same-file caller only (cargo_lock_closure_pins, on build.rs's engine_version_hash_for path); pub for persistent_cache.rs unit tests; audit counts cross-file refs
pub fn parse_cargo_lock_packages(lock_text: &str) -> Vec<(String, String)> {
    parse_cargo_lock_stanzas(lock_text)
        .into_iter()
        .map(|package| (package.name, package.version))
        .collect()
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
/// [`engine_version_hash_for`] extends its parts with the result, replacing the
/// removed whole-file Cargo.lock walk.
#[allow(dead_code)]
pub fn cargo_lock_closure_parts(lock_text: &str, closure: &[&str]) -> Vec<Vec<u8>> {
    let mut parts: Vec<Vec<u8>> = Vec::new();
    for (name, version) in cargo_lock_closure_pins(lock_text, closure) {
        parts.push(name.into_bytes());
        parts.push(version.into_bytes());
    }
    parts
}

/// The canonical `ENGINE_VERSION_HASH` of the reify-eval checkout rooted at
/// `manifest_dir`, together with every path it read.
// build.rs (via include!) reads both fields; the non-test lib build never
// constructs one.
#[allow(dead_code)]
pub struct EngineVersionHash {
    /// 32 lowercase hex chars, as returned by [`compose_engine_version_hash`].
    pub hex: String,
    /// Every file and directory read, for `cargo:rerun-if-changed`.
    pub rerun_paths: Vec<PathBuf>,
}

/// Compute `ENGINE_VERSION_HASH` over `manifest_dir` (reify-eval's
/// `CARGO_MANIFEST_DIR`): every [`contributor_paths`] entry walked by
/// [`walk_contributor`], then reify-eval's closure pins
/// ([`cargo_lock_closure_parts`] over `../../Cargo.lock`, filtered to the
/// names in `engine_hash_closure.txt`), composed by
/// [`compose_engine_version_hash`]. `build.rs` bakes the result into the
/// library; tests run this same function over a mirror of its inputs.
///
/// No cargo metadata is invoked (fragile, offline-hostile, and it can deadlock
/// on the package-cache lock): the closure is the static checked-in manifest,
/// kept a superset (⊇) of the live closure by
/// `tests/infra/test_engine_hash_closure.sh`.
///
/// # Panics
///
/// On a missing contributor or closure-pin input, naming it. A silent skip
/// would shrink the hash input and bake a stale hash unnoticed
/// (`docs/prds/merge-gate-compile-cost.md`).
#[allow(dead_code)]
pub fn engine_version_hash_for(manifest_dir: &Path) -> EngineVersionHash {
    let mut parts: Vec<Vec<u8>> = Vec::new();
    let mut rerun_paths: Vec<PathBuf> = Vec::new();

    for rel in contributor_paths() {
        let path = manifest_dir.join(rel);
        if !path.exists() {
            panic!(
                "ENGINE_VERSION_HASH contributor not found: {} (resolved to {}). \
                 If this file was renamed, moved, or deleted, update \
                 WORKSPACE_CRATE_COVERAGE in crates/reify-eval/src/engine_hash_algo.rs in the same commit.",
                rel,
                path.display()
            );
        }
        let walk = walk_contributor(rel, &path);
        rerun_paths.extend(walk.rerun_paths);
        parts.extend(walk.parts);
    }

    let lock_path = manifest_dir.join("../../Cargo.lock");
    let closure_path = manifest_dir.join("engine_hash_closure.txt");
    for (path, rel) in [
        (&lock_path, "../../Cargo.lock"),
        (&closure_path, "engine_hash_closure.txt"),
    ] {
        if !path.exists() {
            panic!(
                "ENGINE_VERSION_HASH closure-pin input not found: {} (resolved to {}). \
                 If this file was renamed, moved, or deleted, update the closure-pin \
                 wiring in engine_version_hash_for \
                 (crates/reify-eval/src/engine_hash_algo.rs) in the same commit.",
                rel,
                path.display()
            );
        }
        rerun_paths.push(path.clone());
    }
    let read = |path: &Path| {
        std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("ENGINE_VERSION_HASH: cannot read {}: {e}", path.display()))
    };
    let lock_text = read(&lock_path);
    let closure = parse_closure_manifest(&read(&closure_path));
    let closure_refs: Vec<&str> = closure.iter().map(|s| s.as_str()).collect();
    parts.extend(cargo_lock_closure_parts(&lock_text, &closure_refs));

    let part_refs: Vec<&[u8]> = parts.iter().map(|v| v.as_slice()).collect();
    EngineVersionHash {
        hex: compose_engine_version_hash(&part_refs),
        rerun_paths,
    }
}
