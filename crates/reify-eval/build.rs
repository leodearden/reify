// build.rs — computes the canonical ENGINE_VERSION_HASH for reify-eval.
//
// This hash captures the semantic version of the FEA engine: any change to a
// contributor source file will produce a new hash, causing all persistent cache
// entries to miss and be recomputed from scratch. Wire-format changes are
// tracked separately by `ELASTIC_RESULT_FORMAT_VERSION` in `persistent_cache.rs`.
//
// # Contributor categories (per PRD docs/prds/v0_3/persistent-fea-cache.md
//   §"Cache invalidation on engine version")
//
//   1. FEA solver implementation   — crates/reify-solver-elastic (src/ + Cargo.toml)
//   2. Meshing pipeline            — crates/reify-kernel-gmsh (src/ + Cargo.toml + build.rs)
//   3. Stdlib FEA helpers          — crates/reify-stdlib/src/{fea,loads,supports,analysis}.rs
//   4. Per-purpose tolerance impl  — crates/reify-eval/src/tolerance_*.rs,
//                                    engine_tolerance.rs, engine_purposes.rs
//
// # Deferred contributor
//   Materials database: PRD line 59 makes this conditional on materials living
//   in a versioned source file. No such file exists in the repo yet; when one
//   is introduced (e.g. `crates/reify-stdlib/data/materials.toml`), add it to
//   CONTRIBUTORS_RELATIVE below. Adding it will naturally invalidate all existing
//   cache entries (new hash ⇒ miss ⇒ recompute), which is the desired policy.
//
// # Algorithm (mirrors `compose_engine_version_hash` in persistent_cache.rs)
//   For each contributor: emit `cargo:rerun-if-changed`, then collect bytes.
//   Each (path_bytes, file_bytes) pair is framed with a u64 LE length prefix.
//   Including path bytes means renames change the hash even when content is
//   identical — the desired semantics.
//   Single xxh3_128 call over the full buffer → 32-char lowercase hex.
//
// # Safety
//   Missing contributor ⇒ hard panic. A silent skip would silently shrink the
//   contributor set and produce a stale hash without anyone noticing.

use std::path::{Path, PathBuf};

use xxhash_rust::xxh3::xxh3_128;

/// Contributor paths relative to `CARGO_MANIFEST_DIR` (i.e. `crates/reify-eval/`).
/// Each entry is either a single file or a directory to walk recursively.
/// Directories are followed by their contained `.rs` files (sorted by file name
/// for byte-determinism across platforms).
const CONTRIBUTORS_RELATIVE: &[&str] = &[
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
];

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR must be set by cargo");
    let manifest_path = Path::new(&manifest_dir);

    // Re-run this build script whenever it changes itself.
    println!("cargo:rerun-if-changed=build.rs");

    let mut hash_buf: Vec<u8> = Vec::new();

    for rel in CONTRIBUTORS_RELATIVE {
        let path = manifest_path.join(rel);
        if path.is_dir() {
            collect_dir(&path, &path, &mut hash_buf);
        } else if path.is_file() {
            // Emit cargo rerun directive.
            println!("cargo:rerun-if-changed={}", path.display());
            // Frame: path bytes then file bytes, each with u64 LE length prefix.
            let path_bytes = rel.as_bytes();
            push_framed(&mut hash_buf, path_bytes);
            let file_bytes = std::fs::read(&path).unwrap_or_else(|e| {
                panic!(
                    "ENGINE_VERSION_HASH contributor not found or unreadable: {} — {e}",
                    path.display()
                )
            });
            push_framed(&mut hash_buf, &file_bytes);
        } else {
            panic!(
                "ENGINE_VERSION_HASH contributor not found: {} (resolved to {})",
                rel,
                path.display()
            );
        }
    }

    let hash = xxh3_128(&hash_buf);
    println!(
        "cargo:rustc-env=REIFY_ENGINE_VERSION_HASH={:032x}",
        hash
    );
}

/// Recursively walk `dir`, emitting `cargo:rerun-if-changed` for each file and
/// appending (path_relative_to_base_bytes, file_bytes) pairs into `buf`.
///
/// Entries are sorted by name before recursion to ensure byte-determinism across
/// platforms (filesystem iteration order is unspecified by std and varies between
/// ext4, APFS, NTFS, etc.).
fn collect_dir(base: &Path, dir: &Path, buf: &mut Vec<u8>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("ENGINE_VERSION_HASH: cannot read dir {}: {e}", dir.display()))
        .map(|e| {
            e.unwrap_or_else(|e| {
                panic!("ENGINE_VERSION_HASH: dir entry error in {}: {e}", dir.display())
            })
            .path()
        })
        .collect();

    // Sort for determinism.
    entries.sort_by(|a, b| {
        a.file_name()
            .unwrap_or_default()
            .cmp(b.file_name().unwrap_or_default())
    });

    for entry_path in entries {
        if entry_path.is_dir() {
            collect_dir(base, &entry_path, buf);
        } else if entry_path.is_file() {
            println!("cargo:rerun-if-changed={}", entry_path.display());

            // Use path relative to base for the path-frame, so the hash is
            // independent of the absolute workspace location.
            let rel = entry_path
                .strip_prefix(base)
                .unwrap_or(&entry_path)
                .to_string_lossy();
            push_framed(buf, rel.as_bytes());

            let file_bytes = std::fs::read(&entry_path).unwrap_or_else(|e| {
                panic!(
                    "ENGINE_VERSION_HASH: cannot read {}: {e}",
                    entry_path.display()
                )
            });
            push_framed(buf, &file_bytes);
        }
        // Symlinks and other non-file/dir entries are intentionally skipped.
    }
}

/// Append `(len as u64 LE, data)` to `buf`. Mirrors the framing in
/// `compose_engine_version_hash` in `persistent_cache.rs`.
#[inline]
fn push_framed(buf: &mut Vec<u8>, data: &[u8]) {
    buf.extend_from_slice(&(data.len() as u64).to_le_bytes());
    buf.extend_from_slice(data);
}
