//! Guards on what `ENGINE_VERSION_HASH` covers: every source a persisted
//! target runs must move the hash, and every workspace path crate in
//! reify-eval's closure must be explicitly classified. PRD
//! `docs/prds/v0_3/persistent-fea-cache.md` §"Cache invalidation on engine version".

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use crate::engine_hash_algo::{
    EngineVersionHash, LockPackage, engine_version_hash_for, parse_cargo_lock_stanzas,
};

/// Which files implement each persisted target: its trampoline and the code
/// that trampoline calls. Paths are files relative to `crates/reify-eval`.
/// A byte change in any of them must move `ENGINE_VERSION_HASH`, or a stale
/// persisted result would be served.
const PERSISTED_TARGET_SOURCES: &[(&str, &[&str])] = &[
    (
        "solver::elastic_static",
        &[
            "src/compute_targets/elastic_static.rs",
            "src/compute_targets/bc_resolve.rs",
            "src/compute_targets/shell_solve.rs",
            "src/compute_targets/fea_diagnostics.rs",
            "src/compute_targets/mod.rs",
            "src/topology_selectors.rs",
            "../reify-solver-elastic/src/lib.rs",
            "../reify-kernel-gmsh/src/lib.rs",
            "../reify-fdm/src/as_printed.rs",
            "../reify-ir/src/lib.rs",
            "../reify-core/src/lib.rs",
            "../reify-compute-contract/src/lib.rs",
        ],
    ),
    (
        "solver::buckling",
        &[
            "src/compute_targets/buckling.rs",
            "src/compute_targets/elastic_static.rs",
            "src/compute_targets/mod.rs",
            "../reify-solver-elastic/src/lib.rs",
        ],
    ),
    (
        "shell-extract::extract",
        &[
            "src/shell_extract_compute.rs",
            "../reify-shell-extract/src/lib.rs",
        ],
    ),
];

fn real_manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Recreate, under `mirror_root`, exactly the files and directories `real`
/// reports having read, at their repo-relative positions. Returns the mirror's
/// `crates/reify-eval` manifest dir.
fn mirror_hash_inputs(
    real_manifest: &Path,
    real: &EngineVersionHash,
    mirror_root: &Path,
) -> PathBuf {
    for path in &real.rerun_paths {
        let rel = path
            .strip_prefix(real_manifest)
            .expect("every reported path is manifest_dir.join(..)");
        let mirrored = mirror_root.join(repo_relative(rel));
        if path.is_dir() {
            std::fs::create_dir_all(&mirrored).expect("create mirror dir");
        } else {
            std::fs::create_dir_all(mirrored.parent().expect("mirrored file has a parent"))
                .expect("create mirror parent");
            std::fs::copy(path, &mirrored).expect("copy into mirror");
        }
    }
    mirror_root.join("crates/reify-eval")
}

/// `crates/reify-eval/<manifest_relative>`, with each `..` popping a component
/// lexically — the mirror paths do not exist yet, so the OS cannot resolve them.
fn repo_relative(manifest_relative: &Path) -> PathBuf {
    let mut normalised = PathBuf::new();
    for component in Path::new("crates/reify-eval")
        .join(manifest_relative)
        .components()
    {
        match component {
            Component::ParentDir => {
                normalised.pop();
            }
            Component::CurDir => {}
            other => normalised.push(other),
        }
    }
    normalised
}

fn regular_file_count(hash: &EngineVersionHash) -> usize {
    hash.rerun_paths.iter().filter(|p| p.is_file()).count()
}

#[test]
#[should_panic(expected = "ENGINE_VERSION_HASH contributor not found")]
fn engine_version_hash_for_panics_naming_the_first_missing_contributor() {
    let tmp = tempfile::TempDir::new().expect("create temp dir");
    engine_version_hash_for(tmp.path());
}

#[test]
fn engine_version_hash_over_a_mirror_of_exactly_the_files_it_read_reproduces_the_real_hash() {
    let real = engine_version_hash_for(real_manifest_dir());
    assert!(
        real.hex.len() == 32
            && real
                .hex
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
        "hex must be 32 lowercase hex chars, got {:?}",
        real.hex
    );
    for closure_pin_input in ["Cargo.lock", "engine_hash_closure.txt"] {
        assert!(
            real.rerun_paths
                .iter()
                .any(|p| p.file_name().is_some_and(|n| n == closure_pin_input)),
            "the closure-pin input {closure_pin_input} must be among the reported paths"
        );
    }

    let mirror_root = tempfile::TempDir::new().expect("create temp dir");
    let mirror_manifest = mirror_hash_inputs(real_manifest_dir(), &real, mirror_root.path());
    let mirrored = engine_version_hash_for(&mirror_manifest);

    assert_eq!(
        mirrored.hex, real.hex,
        "a mirror of exactly the reported files must reproduce the real hash"
    );
    assert_eq!(regular_file_count(&mirrored), regular_file_count(&real));
}

#[test]
fn parse_cargo_lock_stanzas_reports_each_stanzas_source_and_none_for_path_crates() {
    let lock = r#"# This file is automatically @generated by Cargo.
version = 4

[[package]]
name = "aho-corasick"
version = "1.1.3"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "8e60d3430d3a69478a0e0d6c8b20f5b1a0e0c8a3b71d7d5e0e0c8a3b7"
dependencies = [
 "memchr",
]

[[package]]
name = "reify-path-crate"
version = "0.1.0"
dependencies = [
 "aho-corasick",
]

[[package]]
name = "git-dep"
version = "0.3.0"
source = "git+https://example.invalid/x?rev=abc#abc"

[[patch.unused]]
name = "ghost-should-be-ignored"
version = "9.9.9"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;
    let package = |name: &str, version: &str, source: Option<&str>| LockPackage {
        name: name.to_string(),
        version: version.to_string(),
        source: source.map(str::to_string),
    };
    assert_eq!(
        parse_cargo_lock_stanzas(lock),
        vec![
            package(
                "aho-corasick",
                "1.1.3",
                Some("registry+https://github.com/rust-lang/crates.io-index"),
            ),
            package("reify-path-crate", "0.1.0", None),
            package(
                "git-dep",
                "0.3.0",
                Some("git+https://example.invalid/x?rev=abc#abc"),
            ),
        ],
    );
}

#[test]
fn persisted_target_sources_cover_exactly_the_persistable_targets() {
    let declared: BTreeSet<&str> = PERSISTED_TARGET_SOURCES
        .iter()
        .map(|(target, _)| *target)
        .collect();
    let persistable: BTreeSet<&str> = crate::compute_persist::PERSISTABLE_TARGETS
        .iter()
        .copied()
        .collect();
    let undeclared: Vec<&str> = persistable.difference(&declared).copied().collect();
    let stale: Vec<&str> = declared.difference(&persistable).copied().collect();
    assert!(
        undeclared.is_empty() && stale.is_empty(),
        "PERSISTED_TARGET_SOURCES must name exactly PERSISTABLE_TARGETS.\n\
         Persisted but undeclared: {undeclared:?} — list each target's trampoline and \
         implementing-crate files in PERSISTED_TARGET_SOURCES, and make them hashed via \
         src/engine_hash_algo.rs.\n\
         Declared but not persisted: {stale:?} — remove those rows."
    );
}

#[test]
fn every_persisted_target_source_changes_engine_version_hash_when_one_byte_flips() {
    let real = engine_version_hash_for(real_manifest_dir());
    let mirror_root = tempfile::TempDir::new().expect("create temp dir");
    let mirror_manifest = mirror_hash_inputs(real_manifest_dir(), &real, mirror_root.path());
    let baseline = engine_version_hash_for(&mirror_manifest).hex;

    let mut targets_by_file: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (target, files) in PERSISTED_TARGET_SOURCES {
        for file in *files {
            targets_by_file.entry(*file).or_default().push(*target);
        }
    }

    let mut uncovered: Vec<String> = Vec::new();
    for (file, targets) in &targets_by_file {
        let mirrored = mirror_root.path().join(repo_relative(Path::new(file)));
        if !mirrored.exists() {
            std::fs::create_dir_all(mirrored.parent().expect("mirrored file has a parent"))
                .expect("create mirror parent");
            std::fs::copy(real_manifest_dir().join(file), &mirrored)
                .unwrap_or_else(|e| panic!("copy {file} into the mirror: {e}"));
        }
        let original = std::fs::read(&mirrored).expect("read mirrored source");
        let mut flipped = original.clone();
        match flipped.first_mut() {
            Some(first) => *first ^= 0xFF,
            None => flipped.push(0),
        }
        std::fs::write(&mirrored, &flipped).expect("write flipped source");
        let moved = engine_version_hash_for(&mirror_manifest).hex != baseline;
        std::fs::write(&mirrored, &original).expect("restore mirrored source");
        if !moved {
            uncovered.extend(targets.iter().map(|target| format!("{target}: {file}")));
        }
    }
    assert!(
        uncovered.is_empty(),
        "flipping one byte of these persisted-target sources left ENGINE_VERSION_HASH \
         unchanged, so a change to them would serve stale persisted results:\n{}",
        uncovered.join("\n")
    );
}
