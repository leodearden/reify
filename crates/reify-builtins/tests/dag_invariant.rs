//! PRD decision-3 invariant lock-in: `reify-builtins` must depend on exactly
//! `{reify-core}`.
//!
//! `docs/prds/v0_6/builtin-signature-registry.md` §3 decision 3: "`reify-builtins`
//! depends **only on `reify-core`** (for `Type`); it holds rows, `BuiltinId`,
//! arity/arg-slot specs, and result-type resolvers. It holds **no `Value` and no
//! fn pointers**". The `no Value` half is what this test enforces structurally:
//! `Value` lives in `reify-ir`, so a `reify-ir` dep line is the observable
//! signature of that boundary being crossed. Convention alone would not catch a
//! future author reaching for `reify-ir` to spell an eval-side type.
//!
//! Mirrors `crates/reify-ast/tests/dag_invariant.rs` (the B2 invariant guard):
//! reads `Cargo.toml` directly (no cargo subprocess) and asserts every
//! non-comment line whose trim-start begins with `reify-` names exactly
//! `reify-core`.
//!
//! The `[package] name = "reify-builtins"` line starts with `name`, not
//! `reify-`, so the scan is unambiguous (mirrors the reify-core/reify-ast
//! dag_invariant note).
//!
//! NOTE: line-based scan — misses formulations like `[dependencies."reify-foo"]`
//! table headers, quoted `"reify-foo" = …` entries, or continuation-line inline
//! tables. Full TOML parsing is addressed by `scripts/assert-crate-dag.sh`. This
//! guard catches the common cases and is sufficient as a per-crate fast check.

mod common;
use common::{manifest_dir, resolve_manifest_dir};

fn read_manifest() -> String {
    std::fs::read_to_string(std::path::Path::new(&manifest_dir()).join("Cargo.toml"))
        .expect("failed to read crates/reify-builtins/Cargo.toml")
}

#[test]
fn reify_builtins_depends_only_on_reify_core() {
    let cargo_toml = read_manifest();

    // Collect every non-comment line whose trimmed form starts with "reify-".
    // Dependency entries look like `reify-xxx.workspace = true` or
    // `reify-xxx = { ... }`, so the crate name is the first token on the line.
    let reify_dep_lines: Vec<&str> = cargo_toml
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with('#') && trimmed.starts_with("reify-")
        })
        .collect();

    let only_reify_core = reify_dep_lines
        .iter()
        .all(|line| line.trim_start().starts_with("reify-core"));

    assert!(
        only_reify_core,
        "PRD decision-3 invariant violated: reify-builtins/Cargo.toml must \
         reference ONLY reify-core as a reify-* dependency (no Value, no \
         reify-ir), but found these lines:\n{}",
        reify_dep_lines
            .iter()
            .filter(|line| !line.trim_start().starts_with("reify-core"))
            .copied()
            .collect::<Vec<_>>()
            .join("\n")
    );

    assert!(
        !reify_dep_lines.is_empty(),
        "PRD decision-3 invariant violated: reify-builtins/Cargo.toml must \
         reference reify-core as a dependency, but no reify-* line was found \
         — the dep was likely removed by mistake."
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
        resolve_manifest_dir(Ok("/runtime/worktree/crates/reify-builtins".to_string())),
        "/runtime/worktree/crates/reify-builtins"
    );

    // (b) runtime value absent — falls back to the compile-time env!() bake.
    assert_eq!(
        resolve_manifest_dir(Err(std::env::VarError::NotPresent)),
        env!("CARGO_MANIFEST_DIR")
    );
}

/// Exercises the real runtime seam — `manifest_dir()` itself, not the
/// injected-argument policy pin above — by asserting it resolves (via
/// `std::env::var("CARGO_MANIFEST_DIR")` read at test-execution time) to a
/// directory that actually contains this crate's `Cargo.toml`. A regression
/// that reverted `manifest_dir()` to read only the compile-time `env!()` bake
/// would still pass the pure `resolve_manifest_dir` pin test above (which
/// never calls `manifest_dir()`); this test names the composed wiring
/// directly so a break surfaces with an unambiguous message. See esc-4906-57.
#[test]
fn manifest_dir_resolves_to_a_readable_cargo_toml() {
    let cargo_toml_path = std::path::Path::new(&manifest_dir()).join("Cargo.toml");
    assert!(
        cargo_toml_path.is_file(),
        "manifest_dir() resolved to {cargo_toml_path:?}, which does not \
         contain a readable Cargo.toml — the runtime CARGO_MANIFEST_DIR \
         wiring is broken"
    );
}
