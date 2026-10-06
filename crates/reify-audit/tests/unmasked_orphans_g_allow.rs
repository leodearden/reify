//! Pin: the `pub fn`s that #6424's literal-aware cfg(test) mask-start made
//! visible to `scripts/audit-orphan-producers.sh`, triaged by #6429. Each one
//! carries a `// G-allow:` marker that moves it from `orphans[]` to `allowed[]`.
//!
//! Removal contract: when a pinned fn gains a real cross-file caller it leaves
//! `allowed[]`; delete its row in the commit that wires that caller.
//!
//! These tests assert classification only, never marker prose.
//!
//! A missing python3, git or audit script skips these pins, unless
//! `REIFY_REQUIRE_ORPHAN_AUDIT` is set (non-empty, not `0`); then the skip
//! becomes a panic, matching the sibling `new_orphans_*_g_allow.rs` pin files.

use std::sync::OnceLock;

use reify_test_support::run_orphan_audit;

fn wide_audit() -> Option<&'static serde_json::Value> {
    static AUDIT: OnceLock<Option<serde_json::Value>> = OnceLock::new();
    let audit = AUDIT.get_or_init(|| run_orphan_audit("crates/reify-*/src"));
    let required = std::env::var("REIFY_REQUIRE_ORPHAN_AUDIT")
        .is_ok_and(|flag| !flag.is_empty() && flag != "0");
    assert!(
        audit.is_some() || !required,
        "REIFY_REQUIRE_ORPHAN_AUDIT is set but the orphan audit could not run \
         (python3, git, or scripts/audit-orphan-producers.sh missing)."
    );
    audit.as_ref()
}

fn rows(audit: &serde_json::Value, list: &str, file: &str, name: &str) -> usize {
    audit[list]
        .as_array()
        .unwrap_or_else(|| panic!("audit `{list}` must be an array"))
        .iter()
        .filter(|entry| entry["file"] == file && entry["name"] == name)
        .count()
}

fn likely_cause(orphaned: usize, allowed: usize, name: &str) -> String {
    match (orphaned, allowed) {
        (0, 0) => format!(
            "callers > 0. The audit counts callers per bare fn name, so this is \
             either a wired consumer (delete the pin row) or a reference to a \
             same-named fn elsewhere; `git grep -nw {name} -- 'crates/reify-*/src'` \
             tells which"
        ),
        (0, _) => "duplicate allow-list entries for one (file, name)".to_string(),
        _ => "the `// G-allow:` marker is missing or not on the line directly above \
              `pub fn`, below any attribute"
            .to_string(),
    }
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
                    "  {file}::{name}: orphans[] x{orphaned}, allowed[] x{allowed} \
                     (want x0, x1): {}",
                    likely_cause(orphaned, allowed, name),
                )
            })
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} pin(s) not allow-listed:\n{}",
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

#[test]
fn reify_solver_elastic_unmasked_rows_are_allow_listed() {
    assert_allow_listed(&[
        (
            "crates/reify-solver-elastic/src/resample.rs",
            "resample_nodal_to_grid_instrumented",
        ),
        (
            "crates/reify-solver-elastic/src/resample.rs",
            "resample_multi_nodal_to_grid_instrumented",
        ),
        (
            "crates/reify-solver-elastic/src/resample.rs",
            "classify_grid_misses",
        ),
    ]);
}

#[test]
fn reify_syntax_unmasked_row_is_allow_listed() {
    assert_allow_listed(&[(
        "crates/reify-syntax/src/lib.rs",
        "visit_structure_member_root_exprs",
    )]);
}
