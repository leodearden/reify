// build.rs — computes the canonical ENGINE_VERSION_HASH for reify-eval.
//
// This hash captures the semantic version of the FEA engine: any change to a
// contributor source file will produce a new hash, causing all persistent cache
// entries to miss and be recomputed from scratch. Wire-format changes are
// versioned separately by `ELASTIC_RESULT_FORMAT_VERSION` in `persistent_cache.rs`.
//
// # Algorithm and contributor-walk logic
//
//   The `compose_engine_version_hash` and `walk_contributor` functions are NOT
//   duplicated here. They live in `src/engine_hash_algo.rs`, which is the single
//   source of truth shared between the library crate (via
//   `pub(crate) mod engine_hash_algo;` in `lib.rs`) and this build script (via
//   `include!()` below). Any algorithm change automatically affects both callers.
//
//   `walk_contributor` emits `rerun_paths` entries for BOTH file paths AND
//   directory paths (every directory visited, including the root and all
//   sub-directories). This is the issue-#1 fix: with file-only directives, adding
//   a brand-new source file to a contributor directory silently fails to trigger
//   a rebuild and the new file's bytes are absent from ENGINE_VERSION_HASH.
//   Directory-level directives cause cargo to re-run whenever the directory's
//   child set changes (file added / renamed / removed).
//
// # Contributor categories (per PRD docs/prds/v0_3/persistent-fea-cache.md
//   §"Cache invalidation on engine version")
//
//   1. FEA solver implementation   — crates/reify-solver-elastic (src/ + Cargo.toml)
//   2. Meshing pipeline            — crates/reify-kernel-gmsh (src/ + Cargo.toml + build.rs)
//   3. Stdlib FEA helpers          — crates/reify-stdlib/src/{fea,loads,supports,analysis}.rs
//   4. Per-purpose tolerance impl  — crates/reify-eval/src/tolerance_*.rs,
//                                    engine_tolerance.rs, engine_purposes.rs
//   5. Transitive-dep version pin  — workspace Cargo.lock (../../Cargo.lock)
//
// # Deferred contributor
//   Materials database: PRD line 59 makes this conditional on materials living
//   in a versioned source file. No such file exists in the repo yet; when one
//   is introduced (e.g. `crates/reify-stdlib/data/materials.toml`), add it to
//   CONTRIBUTORS_RELATIVE below. Adding it will naturally invalidate all existing
//   cache entries (new hash ⇒ miss ⇒ recompute), which is the desired policy.
//
// # Safety
//   Missing contributor ⇒ hard panic. A silent skip would silently shrink the
//   contributor set and produce a stale hash without anyone noticing.

// Pull in compose_engine_version_hash, walk_contributor, ContributorWalk,
// and their transitive use statements (std::path::{Path, PathBuf},
// xxhash_rust::xxh3::xxh3_128) from the single shared source file.
// There is NO duplicate algorithm here.
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/engine_hash_algo.rs"
));

fn main() {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by cargo");
    let manifest_path = Path::new(&manifest_dir);

    // Declare has_openvdb as a known cfg so rustc doesn't warn about unknown cfgs.
    println!("cargo::rustc-check-cfg=cfg(has_openvdb)");
    // Enable has_openvdb if OpenVDB native libraries are available.
    if reify_build_utils::find(reify_build_utils::NativeDep::OpenVdb).is_some() {
        println!("cargo:rustc-cfg=has_openvdb");
    }
    // Emit RPATH so test binaries that transitively link libopenvdb resolve it at runtime.
    reify_build_utils::emit_rpath_for_tests(reify_build_utils::NativeDep::OpenVdb);

    // Re-run this build script whenever it changes itself.
    println!("cargo:rerun-if-changed=build.rs");
    // Re-run when the shared algorithm source changes.
    println!("cargo:rerun-if-changed=src/engine_hash_algo.rs");

    let mut all_parts: Vec<Vec<u8>> = Vec::new();

    for rel in CONTRIBUTORS_RELATIVE {
        let path = manifest_path.join(rel);
        if !path.exists() {
            panic!(
                "ENGINE_VERSION_HASH contributor not found: {} (resolved to {}). \
                 If this file was renamed, moved, or deleted, update \
                 CONTRIBUTORS_RELATIVE in crates/reify-eval/src/engine_hash_algo.rs in the same commit.",
                rel,
                path.display()
            );
        }
        let walk = walk_contributor(rel, &path);
        for p in &walk.rerun_paths {
            println!("cargo:rerun-if-changed={}", p.display());
        }
        all_parts.extend(walk.parts);
    }

    let all_refs: Vec<&[u8]> = all_parts.iter().map(|v| v.as_slice()).collect();
    let hash = compose_engine_version_hash(&all_refs);
    println!("cargo:rustc-env=REIFY_ENGINE_VERSION_HASH={hash}");
}
