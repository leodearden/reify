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
//! reads `Cargo.toml` directly (no cargo subprocess) and asserts that every
//! non-comment line whose trim-start begins with `reify-`, in a
//! production-dependency section (see below), names exactly `reify-core`.
//!
//! The `[package] name = "reify-builtins"` line starts with `name`, not
//! `reify-`, so the scan is unambiguous (mirrors the reify-core/reify-ast
//! dag_invariant note).
//!
//! # Which sections the rule applies to
//!
//! Only the sections that participate in the PRODUCTION crate DAG the PRD
//! constrains — `[dependencies]` and `[build-dependencies]`, plus their
//! `[target.'cfg(…)'.…]` qualifications. `[dev-dependencies]` is deliberately
//! OUT of scope: a test-only dep is invisible to every downstream consumer of
//! this crate, so it cannot carry `Value` into `reify-builtins`' public
//! surface, and adding the in-tree `reify-test-support.workspace = true`
//! pattern (as `reify-stdlib` and `reify-compiler` already do) must not trip a
//! decision-3 alarm. The section-blind version of this scan — inherited from
//! the `reify-ast` guard this file mirrors — did trip on exactly that.
//!
//! NOTE: line-based scan — misses formulations like `[dependencies."reify-foo"]`
//! table headers, quoted `"reify-foo" = …` entries, or continuation-line inline
//! tables, and reads the section suffix after the last `.` rather than parsing
//! a real TOML key path. Full TOML parsing is addressed by
//! `scripts/assert-crate-dag.sh`. This guard catches the common cases and is
//! sufficient as a per-crate fast check.

mod common;
use common::{manifest_dir, resolve_manifest_dir};

fn read_manifest() -> String {
    std::fs::read_to_string(std::path::Path::new(&manifest_dir()).join("Cargo.toml"))
        .expect("failed to read crates/reify-builtins/Cargo.toml")
}

/// Does a `[…]` section header name a table whose dependency lines
/// participate in the PRODUCTION crate DAG?
///
/// `[dependencies]` and `[build-dependencies]` do — a build script links into
/// the build graph. `[dev-dependencies]` does not, and neither does any other
/// table (`[package]`, `[features]`, `[lints]`, …). Target-qualified forms
/// (`[target.'cfg(unix)'.dependencies]`) are covered because the comparison is
/// against the suffix after the last `.`.
fn section_is_production_deps(header: &str) -> bool {
    let inner = header.trim().trim_start_matches('[').trim_end_matches(']');
    let last = inner
        .rsplit('.')
        .next()
        .unwrap_or(inner)
        .trim()
        .trim_matches(|c| c == '"' || c == '\'');
    last == "dependencies" || last == "build-dependencies"
}

/// Collect every non-comment line whose trimmed form starts with `reify-`, from
/// the production-dependency sections ONLY.
///
/// Dependency entries look like `reify-xxx.workspace = true` or
/// `reify-xxx = { ... }`, so the crate name is the first token on the line.
fn production_reify_dep_lines(cargo_toml: &str) -> Vec<&str> {
    let mut in_production_section = false;
    let mut out = Vec::new();
    for line in cargo_toml.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') {
            in_production_section = section_is_production_deps(trimmed);
            continue;
        }
        if in_production_section && trimmed.starts_with("reify-") {
            out.push(line);
        }
    }
    out
}

/// `[dependencies]` / `[build-dependencies]` may name only `reify-core`;
/// `[dev-dependencies]` is out of scope (see the module doc).
#[test]
fn section_classification_covers_production_deps_but_not_dev_deps() {
    for header in [
        "[dependencies]",
        "[build-dependencies]",
        "[target.'cfg(unix)'.dependencies]",
        "[target.\"cfg(target_arch = \\\"wasm32\\\")\".build-dependencies]",
    ] {
        assert!(
            section_is_production_deps(header),
            "{header} must be scanned — it feeds the production crate DAG"
        );
    }
    for header in [
        "[dev-dependencies]",
        "[target.'cfg(unix)'.dev-dependencies]",
        "[package]",
        "[features]",
        "[lints.rust]",
    ] {
        assert!(
            !section_is_production_deps(header),
            "{header} must NOT be scanned by the decision-3 rule"
        );
    }

    // The behaviour that motivated the fix: a test-only reify-* dep is legal.
    let manifest = "\
[package]
name = \"reify-builtins\"

[dependencies]
reify-core.workspace = true

[dev-dependencies]
reify-test-support.workspace = true
";
    assert_eq!(
        production_reify_dep_lines(manifest),
        vec!["reify-core.workspace = true"],
        "a [dev-dependencies] reify-* entry must not be collected"
    );
}

#[test]
fn reify_builtins_depends_only_on_reify_core() {
    let cargo_toml = read_manifest();

    let reify_dep_lines = production_reify_dep_lines(&cargo_toml);

    let only_reify_core = reify_dep_lines
        .iter()
        .all(|line| line.trim_start().starts_with("reify-core"));

    assert!(
        only_reify_core,
        "PRD decision-3 invariant violated: reify-builtins/Cargo.toml must \
         reference ONLY reify-core as a reify-* PRODUCTION dependency \
         ([dependencies]/[build-dependencies]; no Value, no reify-ir), but \
         found these lines:\n{}",
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
         in [dependencies]/[build-dependencies] — the dep was likely removed \
         by mistake."
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
