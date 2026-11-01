//! Pin: the `reify-git-env` leaf is swept by `scripts/audit-orphan-producers.sh`
//! like any other crate, and every `pub fn` in it either has a cross-crate
//! production caller or carries a `// G-allow:` marker.
//!
//! Hosted here rather than in the leaf because wide-scope orphan pins live in
//! this crate's tests, and the std-only leaf takes no `reify-test-support`
//! dev-dependency. The scope is the WIDE `crates/reify-*/src` corpus: the audit
//! counts callers only inside its scope, so a run scoped to a single-file crate
//! would report every `pub fn` in it as an orphan.
//!
//! The leaf's `allowed[]` rows double as the witness that the crate is swept at
//! all; an excluded crate would produce none. Removal contract: when a pinned
//! fn gains a real cross-crate production caller it leaves `allowed[]`; delete
//! its row in the commit that wires that caller.
//!
//! These tests assert classification only, never marker prose.
//!
//! A missing python3, git or audit script skips these pins, unless
//! `REIFY_REQUIRE_ORPHAN_AUDIT` is set; see [`wide_orphan_audit`].

use reify_test_support::wide_orphan_audit;

const LEAF_SRC: &str = "crates/reify-git-env/src/";
const LEAF_LIB: &str = "crates/reify-git-env/src/lib.rs";

fn entries<'a>(audit: &'a serde_json::Value, list: &str) -> &'a [serde_json::Value] {
    audit[list]
        .as_array()
        .unwrap_or_else(|| panic!("audit `{list}` must be an array"))
}

fn rows(audit: &serde_json::Value, list: &str, file: &str, name: &str) -> usize {
    entries(audit, list)
        .iter()
        .filter(|entry| entry["file"] == file && entry["name"] == name)
        .count()
}

#[test]
fn reify_git_env_has_no_orphan_rows() {
    let Some(audit) = wide_orphan_audit() else {
        return;
    };
    let offenders: Vec<String> = entries(audit, "orphans")
        .iter()
        .filter_map(|entry| {
            let file = entry["file"].as_str()?;
            file.starts_with(LEAF_SRC).then(|| {
                let name = entry["name"].as_str().unwrap_or("<unnamed>");
                format!("  {file}::{name} (callers: {})", entry["callers"])
            })
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "{} orphan row(s) under {LEAF_SRC}. Each needs a cross-crate production \
         caller, or a `// G-allow:` marker on the line directly above `pub fn`, \
         below any attribute:\n{}",
        offenders.len(),
        offenders.join("\n"),
    );
}

#[test]
fn reify_git_env_test_side_fns_are_allow_listed() {
    let Some(audit) = wide_orphan_audit() else {
        return;
    };
    let failures: Vec<String> = ["sanitize", "removed_vars"]
        .iter()
        .filter_map(|&name| {
            let orphaned = rows(audit, "orphans", LEAF_LIB, name);
            let allowed = rows(audit, "allowed", LEAF_LIB, name);
            (orphaned != 0 || allowed != 1).then(|| {
                format!(
                    "  {LEAF_LIB}::{name}: orphans[] x{orphaned}, allowed[] x{allowed} \
                     (want x0, x1)"
                )
            })
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} pin(s) not allow-listed. allowed[] x0 with orphans[] x0 means the fn \
         gained a caller (delete its row) or the leaf is no longer swept; \
         orphans[] x1 means the `// G-allow:` marker is missing or misplaced:\n{}",
        failures.len(),
        failures.join("\n"),
    );
}
