//! B2 invariant lock-in: `reify-ast` must depend on exactly `{reify-core}` and
//! must have zero tree-sitter dependencies.
//!
//! Reads `Cargo.toml` directly (no cargo subprocess) and asserts:
//! (a) every non-comment line whose trim-start begins with `reify-` names
//!     exactly `reify-core` — fails with a listing of offending lines if any
//!     other `reify-*` dep sneaks in or `reify-core` is missing;
//! (b) no non-comment line whose trim-start begins with `tree-sitter` exists.
//!
//! The `[package] name = "reify-ast"` line starts with `name`, not `reify-`,
//! so the scan is unambiguous (mirrors the reify-core/tests/dag_invariant.rs
//! note).
//!
//! PRD §8 B2: "reify-ast intra-workspace deps == {reify-core}; no tree-sitter dep".
//! The workspace-wide permanent assertion (`scripts/assert-crate-dag.sh`)
//! arrives under task η per PRD §10.

#[test]
fn reify_ast_depends_only_on_reify_core() {
    let cargo_toml = std::fs::read_to_string(
        concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
    )
    .expect("failed to read crates/reify-ast/Cargo.toml");

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

    // B2(a): the only allowed reify-* dependency is reify-core.
    let only_reify_core = reify_dep_lines
        .iter()
        .all(|line| line.trim_start().starts_with("reify-core"));

    assert!(
        only_reify_core,
        "B2 invariant violated: reify-ast/Cargo.toml must reference ONLY \
         reify-core as a reify-* dependency, but found these lines:\n{}",
        reify_dep_lines
            .iter()
            .filter(|line| !line.trim_start().starts_with("reify-core"))
            .copied()
            .collect::<Vec<_>>()
            .join("\n")
    );

    assert!(
        !reify_dep_lines.is_empty(),
        "B2 invariant violated: reify-ast/Cargo.toml must reference reify-core \
         as a dependency, but no reify-* line was found — the dep was likely \
         removed by mistake."
    );
}

#[test]
fn reify_ast_has_no_tree_sitter_dependency() {
    let cargo_toml = std::fs::read_to_string(
        concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
    )
    .expect("failed to read crates/reify-ast/Cargo.toml");

    // Collect every non-comment line whose trimmed form starts with "tree-sitter".
    let tree_sitter_lines: Vec<&str> = cargo_toml
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with('#') && trimmed.starts_with("tree-sitter")
        })
        .collect();

    assert!(
        tree_sitter_lines.is_empty(),
        "B2 invariant violated: reify-ast/Cargo.toml must not reference any \
         tree-sitter dependency, but found these lines:\n{}",
        tree_sitter_lines.join("\n")
    );
}
