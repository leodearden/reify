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
