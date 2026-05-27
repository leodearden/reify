//! B3 invariant lock-in: `reify-ir` must depend on exactly `{reify-core, reify-ast}`
//! and on no other intra-workspace `reify-*` crate.
//!
//! Reads `Cargo.toml` directly (no cargo subprocess) and asserts:
//! (a) every non-comment line whose trim-start begins with `reify-` names either
//!     `reify-core` or `reify-ast` — fails with a listing of offending lines if
//!     any other `reify-*` dep sneaks in;
//! (b) both `reify-core` AND `reify-ast` lines are present — fails if either dep
//!     is removed by mistake.
//!
//! The `[package] name = "reify-ir"` line starts with `name`, not `reify-`,
//! so the scan is unambiguous (mirrors reify-core/tests/dag_invariant.rs and
//! reify-ast/tests/dag_invariant.rs).
//!
//! PRD §8 B3: "reify-ir intra-workspace deps ⊆ {reify-core, reify-ast}".
//! The workspace-wide permanent assertion (`scripts/assert-crate-dag.sh`)
//! arrives under task η per PRD §10.
//!
//! NOTE: line-based scan — misses formulations like `[dependencies."reify-foo"]`
//! table headers, quoted `"reify-foo" = …` entries, or continuation-line inline
//! tables. Full TOML parsing is addressed by `scripts/assert-crate-dag.sh` under
//! task η. This guard catches the common cases and is sufficient as a per-crate
//! fast check.

const ALLOWED: &[&str] = &["reify-core", "reify-ast"];

#[test]
fn reify_ir_depends_only_on_reify_core_and_reify_ast() {
    let cargo_toml = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/Cargo.toml"
    ))
    .expect("failed to read crates/reify-ir/Cargo.toml");

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

    // B3(a): the only allowed reify-* dependencies are reify-core and reify-ast.
    let offending: Vec<&str> = reify_dep_lines
        .iter()
        .copied()
        .filter(|line| {
            let trimmed = line.trim_start();
            !ALLOWED.iter().any(|allowed| {
                // Require a word-boundary character after the crate name so that
                // a hypothetical `reify-core-other` dep does not falsely pass the
                // allow-list (`.` for `.workspace`, ` ` for ` = { … }`, `=` for
                // `={}` inline tables).
                trimmed.starts_with(&format!("{allowed}."))
                    || trimmed.starts_with(&format!("{allowed} "))
                    || trimmed.starts_with(&format!("{allowed}="))
            })
        })
        .collect();

    assert!(
        offending.is_empty(),
        "B3 invariant violated: reify-ir/Cargo.toml must reference ONLY \
         reify-core and reify-ast as reify-* dependencies, but found these \
         offending lines:\n{}",
        offending.join("\n")
    );
}

#[test]
fn reify_ir_has_both_reify_core_and_reify_ast_dependencies() {
    let cargo_toml = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/Cargo.toml"
    ))
    .expect("failed to read crates/reify-ir/Cargo.toml");

    for required in ALLOWED {
        let found = cargo_toml.lines().any(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with('#') && trimmed.starts_with(required)
        });

        assert!(
            found,
            "B3 invariant violated: reify-ir/Cargo.toml must reference \
             `{required}` as a dependency, but no `{required}` line was found \
             — the dep was likely removed by mistake."
        );
    }
}
