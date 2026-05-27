//! B1 invariant lock-in: `reify-core` must have zero `reify-*` dependencies.
//!
//! Shells out to `cargo metadata` and asserts that none of `reify-core`'s
//! declared dependencies have a name starting with `"reify-"`.
//!
//! This test provides a fast-feedback regression guard within the crate's
//! own test suite. The workspace-wide permanent assertion
//! (`scripts/assert-crate-dag.sh`) arrives under task η per PRD §10.

use std::process::Command;

#[test]
fn reify_core_has_no_reify_star_dependencies() {
    // Use the Cargo binary that built this test so we always match the
    // workspace's toolchain (avoids PATH-mismatch issues in CI).
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());

    // Anchor the manifest path relative to this test file's directory so the
    // test runs correctly from any working directory.
    let manifest_path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");

    let output = Command::new(&cargo)
        .args([
            "metadata",
            "--format-version=1",
            "--no-deps",
            "--manifest-path",
            manifest_path,
        ])
        .output()
        .expect("failed to invoke `cargo metadata`");

    assert!(
        output.status.success(),
        "`cargo metadata` failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("failed to parse `cargo metadata` JSON");

    // Locate the reify-core package entry.
    let packages = metadata["packages"]
        .as_array()
        .expect("`packages` is not an array");

    let reify_core_pkg = packages
        .iter()
        .find(|pkg| pkg["name"].as_str() == Some("reify-core"))
        .expect("reify-core package not found in `cargo metadata` output");

    // Check that no dependency name starts with "reify-".
    let deps = reify_core_pkg["dependencies"]
        .as_array()
        .expect("`dependencies` is not an array");

    let reify_deps: Vec<&str> = deps
        .iter()
        .filter_map(|dep| dep["name"].as_str())
        .filter(|name| name.starts_with("reify-"))
        .collect();

    assert!(
        reify_deps.is_empty(),
        "B1 invariant violated: reify-core must have zero reify-* dependencies, \
         but found: {:?}",
        reify_deps
    );
}
