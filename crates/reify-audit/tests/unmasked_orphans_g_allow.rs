//! Pin: the `pub fn`s that #6424's literal-aware cfg(test) mask-start made
//! visible to `scripts/audit-orphan-producers.sh`, triaged by #6429. Each one
//! carries a `// G-allow:` marker that moves it from `orphans[]` to `allowed[]`.
//!
//! Removal contract: when a pinned fn gains a real cross-file caller it leaves
//! `allowed[]`; delete its row in the commit that wires that caller.
//!
//! These tests assert classification only, never marker prose.

use std::sync::OnceLock;

use reify_test_support::run_orphan_audit;

fn wide_audit() -> Option<&'static serde_json::Value> {
    static AUDIT: OnceLock<Option<serde_json::Value>> = OnceLock::new();
    AUDIT
        .get_or_init(|| run_orphan_audit("crates/reify-*/src"))
        .as_ref()
}

fn rows(audit: &serde_json::Value, list: &str, file: &str, name: &str) -> usize {
    audit[list]
        .as_array()
        .unwrap_or_else(|| panic!("audit `{list}` must be an array"))
        .iter()
        .filter(|entry| entry["file"] == file && entry["name"] == name)
        .count()
}

fn assert_allow_listed(pins: &[(&str, &str)]) {
    let Some(audit) = wide_audit() else { return };
    let failures: Vec<String> = pins
        .iter()
        .filter_map(|&(file, name)| {
            let orphaned = rows(audit, "orphans", file, name);
            let allowed = rows(audit, "allowed", file, name);
            (orphaned != 0 || allowed != 1).then(|| {
                format!(
                    "  {file}::{name}: orphans[] x{orphaned}, allowed[] x{allowed} (want x0, x1)"
                )
            })
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} pin(s) not allow-listed:\n{}\n\
         The `// G-allow:` marker must sit on the line directly above `pub fn`, \
         below any attribute.",
        failures.len(),
        failures.join("\n"),
    );
}

#[test]
fn reify_eval_unmasked_rows_are_allow_listed() {
    assert_allow_listed(&[
        (
            "crates/reify-eval/src/engine_hash_algo.rs",
            "walk_contributor",
        ),
        (
            "crates/reify-eval/src/engine_hash_algo.rs",
            "parse_cargo_lock_packages",
        ),
        (
            "crates/reify-eval/src/geometry_ops.rs",
            "gate_query_capability",
        ),
        ("crates/reify-eval/src/geometry_ops.rs", "eval_sub_pose"),
    ]);
}

#[test]
fn reify_mesh_morph_unmasked_rows_are_allow_listed() {
    assert_allow_listed(&[
        (
            "crates/reify-mesh-morph/src/diagnostics.rs",
            "reset_for_test",
        ),
        ("crates/reify-mesh-morph/src/stats.rs", "reset_for_test"),
    ]);
}
