//! B1 invariant lock-in: `reify-core` must have zero `reify-*` dependencies.
//!
//! Reads `Cargo.toml` directly and asserts that no dependency key starts with
//! `"reify-"`. This is faster than shelling out to `cargo metadata` and works
//! in offline / restricted environments.
//!
//! In Cargo.toml, dependency entries appear as lines of the form
//! `reify-xxx.workspace = true` or `reify-xxx = { ... }` — i.e. the crate name
//! is the first token on the line. The package `name = "reify-core"` line starts
//! with `name`, not `reify-`, so the scan is unambiguous.
//!
//! The workspace-wide permanent assertion (`scripts/assert-crate-dag.sh`)
//! arrives under task η per PRD §10.

#[test]
fn reify_core_has_no_reify_star_dependencies() {
    let cargo_toml = std::fs::read_to_string(
        concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
    )
    .expect("failed to read crates/reify-core/Cargo.toml");

    let reify_dep_lines: Vec<&str> = cargo_toml
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with('#') && trimmed.starts_with("reify-")
        })
        .collect();

    assert!(
        reify_dep_lines.is_empty(),
        "B1 invariant violated: reify-core/Cargo.toml must not reference any \
         reify-* dependency, but found these lines:\n{}",
        reify_dep_lines.join("\n")
    );
}

/// Pins the warm-lane CoW-reuse fix: the manifest-dir resolution policy must
/// prefer the runtime `CARGO_MANIFEST_DIR` (correct for whatever worktree is
/// actually running the test) over the compile-time `env!()` bake (which goes
/// stale when a seeded warm-lane `target/` is reused from a since-deleted
/// worktree). See esc-4906-57.
#[test]
fn resolve_manifest_dir_prefers_runtime_then_compile_time() {
    // (a) runtime value present — returned verbatim, not the compile-time bake.
    assert_eq!(
        resolve_manifest_dir(Ok("/runtime/worktree/crates/reify-core".to_string())),
        "/runtime/worktree/crates/reify-core"
    );

    // (b) runtime value absent — falls back to the compile-time env!() bake.
    assert_eq!(
        resolve_manifest_dir(Err(std::env::VarError::NotPresent)),
        env!("CARGO_MANIFEST_DIR")
    );
}
