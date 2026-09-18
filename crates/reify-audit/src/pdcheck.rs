//! PDCHECK — the `delivered_checks` dead-path lane.
//!
//! A task's `metadata.delivered_checks` rows are the capability contract its
//! dependents block on: the orchestrator runs each `kind: grep` row and gates
//! the task's done-flip on the result. A row whose pathspec no longer resolves
//! is therefore a latent outage, and WHICH outage depends on the row's
//! `expect` polarity, because both readings of the runner's rc=1 are silent:
//!
//! - `expect: present` — rc=1 on an empty pathspec reads as FAILED, so every
//!   dependent blocks forever at `DEP_CAPABILITY_NOT_DELIVERED`.
//! - `expect: absent` — the identical rc=1 reads as PASSED, so the check
//!   succeeds while asserting nothing.
//!
//! # The ANY-match quantifier
//!
//! A multi-`paths` row is run as ONE
//! `git grep -E -e <pattern> <ref> -- <paths...>`, so it matches when ANY path
//! matches; it is NOT a per-path conjunction. One dead path among live ones
//! therefore leaves the row perfectly satisfiable. This lane consequently
//! quantifies over the WHOLE row — every path must be absent — and yields at
//! most one verdict per row. Quantifying per path instead would flag every
//! multi-path row carrying a single stale entry and drown the real signal.
//!
//! That contract lives in dark-factory
//! (`orchestrator/src/orchestrator/delivered_checks.py::_run_grep_check`) and
//! is stated here because it cannot be re-derived from this tree, which parses
//! `delivered_checks` nowhere else.

use std::collections::HashSet;

/// Wire spelling of the only `kind` this lane reads. `script` and `manual`
/// rows carry no grep pathspec, so their `paths` assert nothing about path
/// resolvability.
const GREP_KIND: &str = "grep";
const EXPECT_PRESENT: &str = "present";
const EXPECT_ABSENT: &str = "absent";

/// What is wrong with a row whose every path is dead. The two arms are the
/// `expect`-polarity asymmetry described in the module doc — one is an outage,
/// the other a silent hole — so they are different defects, not one defect at
/// two severities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// `expect: present` against a wholly dead pathspec: rc=1 reads as FAILED
    /// and blocks every dependent at mark-done.
    Unsatisfiable,
    /// `expect: absent` against a wholly dead pathspec: rc=1 reads as PASSED,
    /// so the check succeeds while asserting nothing.
    VacuousAbsent,
}

/// One `metadata.delivered_checks` row, holding only what this lane reads.
///
/// Parsed permissively: the producer of this JSON lives in another repo, so a
/// missing, null or wrongly-typed field lands as `None` / empty rather than
/// failing the row. Every such shape is inert at [`classify_row`], which is
/// what keeps the lane from inventing a finding on a row it cannot read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveredCheckRow {
    /// The row's `name` — the key diagnostic, and the handle a fixer needs to
    /// locate the row inside the task's metadata.
    pub name: String,
    pub kind: Option<String>,
    pub expect: Option<String>,
    pub pattern: Option<String>,
    pub paths: Vec<String>,
}

impl DeliveredCheckRow {
    /// `None` only when `value` is not a JSON object — the one shape that is
    /// not a row at all. Every other malformation is absorbed into a field.
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        let obj = value.as_object()?;
        let text = |key: &str| obj.get(key).and_then(|v| v.as_str()).map(str::to_string);
        Some(Self {
            name: text("name").unwrap_or_default(),
            kind: text("kind"),
            expect: text("expect"),
            pattern: text("pattern"),
            paths: obj
                .get("paths")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

/// The lane's whole predicate: one row against the tracked-file set.
///
/// `Option<Verdict>` rather than a collection because a finding is per ROW —
/// see the ANY-match quantifier in the module doc. Path membership is decided
/// by [`crate::ptodo::path_present_in_tracked`] rather than a bare
/// `tracked.contains`, so a trailing-slash or DIRECTORY pathspec that still
/// holds tracked files counts as present; sharing that predicate with PTODO's
/// ζ lane is also what stops the two lanes' membership tests from drifting.
pub fn classify_row(row: &DeliveredCheckRow, tracked: &HashSet<String>) -> Option<Verdict> {
    if row.kind.as_deref() != Some(GREP_KIND) {
        return None;
    }
    let verdict = match row.expect.as_deref() {
        Some(EXPECT_PRESENT) => Verdict::Unsatisfiable,
        Some(EXPECT_ABSENT) => Verdict::VacuousAbsent,
        // An absent or unrecognised polarity says nothing about how the
        // runner's rc=1 would be read, so there is no defect to name.
        _ => return None,
    };
    // An EMPTY pathspec greps the whole tree, so it is never a dead-path row;
    // and one live path satisfies the row under the ANY-match rule.
    if row.paths.is_empty()
        || row
            .paths
            .iter()
            .any(|p| crate::ptodo::path_present_in_tracked(p, tracked))
    {
        return None;
    }
    Some(verdict)
}

#[cfg(test)]
mod tests {
    use super::{DeliveredCheckRow, Verdict, classify_row};
    use serde_json::json;
    use std::collections::HashSet;

    fn tracked(paths: &[&str]) -> HashSet<String> {
        paths.iter().map(|s| s.to_string()).collect()
    }

    fn row(value: serde_json::Value) -> DeliveredCheckRow {
        DeliveredCheckRow::from_json(&value).expect("a JSON object must parse into a row")
    }

    const LIVE: &str = "crates/reify-ir/src/arg_acceptance.rs";
    const DEAD: &str = "crates/reify-eval/src/arg_acceptance.rs";

    /// THE LOAD-BEARING CORRECTNESS RULE. A multi-`paths` grep row is run as a
    /// SINGLE `git grep -E -e <pattern> <ref> -- <paths...>`, so it is an
    /// ANY-match across all paths. One dead path among live ones therefore
    /// leaves the row perfectly satisfiable and must NOT be flagged — a
    /// per-path lane would flood the sweep with false positives on every
    /// multi-path row carrying one stale entry.
    #[test]
    fn mixed_dead_and_live_paths_under_expect_present_is_satisfiable() {
        let r = row(json!({
            "name": "angle-spec-absent-today",
            "kind": "grep",
            "expect": "present",
            "pattern": "pub fn angle_spec",
            "paths": [DEAD, LIVE],
        }));
        assert_eq!(
            classify_row(&r, &tracked(&[LIVE])),
            None,
            "a live path in the row still satisfies the ANY-match grep"
        );
    }

    /// The loud half: rc=1 on a wholly dead pathspec reads as FAILED, so every
    /// dependent of the task blocks forever at mark-done.
    #[test]
    fn all_paths_dead_under_expect_present_is_unsatisfiable() {
        let r = row(json!({
            "name": "angle-spec-absent-today",
            "kind": "grep",
            "expect": "present",
            "pattern": "pub fn angle_spec",
            "paths": [DEAD],
        }));
        assert_eq!(
            classify_row(&r, &tracked(&[LIVE])),
            Some(Verdict::Unsatisfiable),
        );
    }

    /// The invisible half: the identical rc=1 reads as PASSED under
    /// `expect: absent`, so the check succeeds while asserting nothing.
    #[test]
    fn all_paths_dead_under_expect_absent_is_vacuous() {
        let r = row(json!({
            "name": "bare-angle-resolver-retired",
            "kind": "grep",
            "expect": "absent",
            "pattern": "fn resolve_bare_angle",
            "paths": [DEAD],
        }));
        assert_eq!(
            classify_row(&r, &tracked(&[LIVE])),
            Some(Verdict::VacuousAbsent),
        );
    }

    /// Same ANY-match quantifier on the other polarity: a live path means the
    /// grep still has something to assert over, so the row is not vacuous.
    #[test]
    fn mixed_dead_and_live_paths_under_expect_absent_is_not_vacuous() {
        let r = row(json!({
            "name": "bare-angle-resolver-retired",
            "kind": "grep",
            "expect": "absent",
            "pattern": "fn resolve_bare_angle",
            "paths": [DEAD, LIVE],
        }));
        assert_eq!(classify_row(&r, &tracked(&[LIVE])), None);
    }

    #[test]
    fn all_paths_live_yields_no_verdict_under_either_polarity() {
        for expect in ["present", "absent"] {
            let r = row(json!({
                "name": "healthy",
                "kind": "grep",
                "expect": expect,
                "pattern": "pub fn angle_spec",
                "paths": [LIVE],
            }));
            assert_eq!(
                classify_row(&r, &tracked(&[LIVE])),
                None,
                "a row whose paths all resolve is never a dead-path finding (expect: {expect})"
            );
        }
    }

    /// `kind: script` and `kind: manual` rows are not greps — their paths are
    /// not a pathspec and carry no resolvability claim. Both are live shapes:
    /// tasks #5752 and #5761 each carry a real `kind: script` row.
    #[test]
    fn non_grep_rows_are_ignored_regardless_of_paths() {
        for kind in ["script", "manual"] {
            let r = row(json!({
                "name": "drift-guard-registrations-same-diff",
                "kind": kind,
                "expect": "present",
                "pattern": null,
                "paths": [DEAD],
            }));
            assert_eq!(
                classify_row(&r, &tracked(&[LIVE])),
                None,
                "kind: {kind} carries no grep pathspec"
            );
        }
    }

    /// A DIRECTORY pathspec resolves as PRESENT whenever a tracked file lives
    /// under it. `crates/reify-eval/tests` and `crates/reify-eval/tests/golden`
    /// are live multi-path row targets on #5796/#5781, and a directory is never
    /// itself a member of the `git ls-files` set — so treating one as absent
    /// would false-positive on every such row.
    #[test]
    fn directory_and_trailing_slash_pathspecs_count_as_present() {
        let set = tracked(&["crates/reify-eval/tests/golden/angle.rs"]);
        for pathspec in [
            "crates/reify-eval/tests",
            "crates/reify-eval/tests/",
            "crates/reify-eval/tests/golden",
        ] {
            let r = row(json!({
                "name": "universe-independent-of-assertion-target",
                "kind": "grep",
                "expect": "present",
                "pattern": "GEOMETRY_FUNCTION_NAMES",
                "paths": [pathspec],
            }));
            assert_eq!(
                classify_row(&r, &set),
                None,
                "directory pathspec '{pathspec}' still contains tracked files"
            );
        }
    }

    /// The producer of this JSON lives in another repo, so every field must be
    /// optional at the parse and inert at the classifier.
    #[test]
    fn degenerate_rows_yield_no_verdict_without_panicking() {
        let empty_tracked = tracked(&[]);
        let degenerate = [
            // An empty pathspec greps the WHOLE tree, so it is never a
            // dead-path row however the polarity reads.
            json!({"name": "n", "kind": "grep", "expect": "present", "paths": []}),
            json!({"name": "n", "kind": "grep", "expect": "present"}),
            json!({"name": "n", "kind": "grep", "paths": [DEAD]}),
            json!({"name": "n", "kind": "grep", "expect": null, "paths": [DEAD]}),
            json!({"name": "n", "kind": "grep", "expect": "sometimes", "paths": [DEAD]}),
            json!({"name": "n", "expect": "present", "paths": [DEAD]}),
            json!({"name": "n", "kind": null, "expect": "present", "paths": [DEAD]}),
            json!({}),
        ];
        for value in degenerate {
            let r = row(value.clone());
            assert_eq!(
                classify_row(&r, &empty_tracked),
                None,
                "degenerate row must yield no verdict: {value}"
            );
        }
    }

    #[test]
    fn a_row_that_is_not_an_object_does_not_parse() {
        for value in [json!("grep"), json!(7), json!([DEAD]), json!(null)] {
            assert_eq!(
                DeliveredCheckRow::from_json(&value),
                None,
                "only a JSON object is a delivered_checks row: {value}"
            );
        }
    }

    /// The finding count is per ROW, not per path: `Option<Verdict>` makes
    /// "at most one" unrepresentable otherwise, and this pins that choice
    /// against a future refactor to `Vec<Verdict>`.
    #[test]
    fn a_row_with_several_dead_paths_yields_exactly_one_verdict() {
        let r = row(json!({
            "name": "angle-spec-absent-today",
            "kind": "grep",
            "expect": "present",
            "pattern": "pub fn angle_spec",
            "paths": [DEAD, "crates/reify-eval/src/gone.rs", "crates/reify-eval/src/also_gone.rs"],
        }));
        assert_eq!(
            classify_row(&r, &tracked(&[LIVE])),
            Some(Verdict::Unsatisfiable),
        );
    }
}
