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

use crate::task_rows::{PathHistory, metadata_array, non_terminal_master_tasks, path_present_in_tracked};
use crate::{EvidenceRef, Finding, GitCommit, GitOps, Pattern, Severity};
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
enum Verdict {
    /// `expect: present` against a wholly dead pathspec: rc=1 reads as FAILED
    /// and blocks every dependent at mark-done.
    Unsatisfiable,
    /// `expect: absent` against a wholly dead pathspec: rc=1 reads as PASSED,
    /// so the check succeeds while asserting nothing.
    VacuousAbsent,
}

impl Verdict {
    /// The finding `kind`, carried as a stable summary prefix rather than as a
    /// per-kind [`Pattern`] variant — PTODO's convention.
    fn kind(self) -> &'static str {
        match self {
            Verdict::Unsatisfiable => "delivered-check-unsatisfiable-path",
            Verdict::VacuousAbsent => "delivered-check-vacuous-absent-path",
        }
    }

    /// High for the outage (a permanently blocked dependent), Medium for the
    /// silent hole (a check that passes while asserting nothing).
    fn severity(self) -> Severity {
        match self {
            Verdict::Unsatisfiable => Severity::High,
            Verdict::VacuousAbsent => Severity::Medium,
        }
    }
}

/// One `metadata.delivered_checks` row, holding only what this lane reads.
///
/// Parsed permissively: the producer of this JSON lives in another repo, so a
/// missing, null or wrongly-typed field lands as `None` / empty rather than
/// failing the row. Every such shape is inert at [`classify_row`], which is
/// what keeps the lane from inventing a finding on a row it cannot read.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DeliveredCheckRow {
    /// The row's `name` — the key diagnostic, and the handle a fixer needs to
    /// locate the row inside the task's metadata.
    name: String,
    kind: Option<String>,
    expect: Option<String>,
    pattern: Option<String>,
    paths: Vec<String>,
}

impl DeliveredCheckRow {
    /// `None` only when `value` is not a JSON object — the one shape that is
    /// not a row at all. Every other malformation is absorbed into a field.
    fn from_json(value: &serde_json::Value) -> Option<Self> {
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

/// Whether `pathspec` carries git pathspec MAGIC: a leading `:` (`:(exclude)`,
/// `:!`, `:/`, `:^`) or an fnmatch wildcard. Git resolves both;
/// [`path_present_in_tracked`] models only exact-match and directory-prefix
/// membership and cannot.
fn is_pathspec_magic(pathspec: &str) -> bool {
    pathspec.starts_with(':') || pathspec.contains(['*', '?', '['])
}

/// The lane's whole predicate: one row against the tracked-file set.
///
/// `Option<Verdict>` rather than a collection because a finding is per ROW —
/// see the ANY-match quantifier in the module doc. Path membership is decided
/// by [`path_present_in_tracked`] rather than a bare `tracked.contains`, so a
/// trailing-slash or DIRECTORY pathspec that still holds tracked files counts
/// as present; sharing that predicate with PTODO's ζ lane is also what stops
/// the two lanes' membership tests from drifting.
///
/// A row carrying pathspec magic is UNCLASSIFIABLE here and stays silent: the
/// membership test would call `crates/reify-ir/src/*.rs` absent while git greps
/// it fine, which is this lane's only false-positive class — and it would land
/// on the High kind. Silence is the fail-safe direction.
fn classify_row(row: &DeliveredCheckRow, tracked: &HashSet<String>) -> Option<Verdict> {
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
    // one live path satisfies the row under the ANY-match rule; and one magic
    // path makes the whole row unreadable to this lane.
    if row.paths.is_empty()
        || row
            .paths
            .iter()
            .any(|p| is_pathspec_magic(p) || path_present_in_tracked(p, tracked))
    {
        return None;
    }
    Some(verdict)
}

/// A dead path of a flagged row, together with what git knows about it: the
/// last commit that touched it, and the still-tracked path it was renamed to
/// if there is one.
struct DeadPath {
    path: String,
    commit: GitCommit,
    rename_target: Option<String>,
}

/// The lane: for each non-terminal master task, classify every
/// `metadata.delivered_checks` row against the tracked-file set.
///
/// `target_task_id` narrows the sweep to one task — what `--task <id>` means
/// for a task-state-shaped detector, and the shape someone unblocking a single
/// stuck dependent actually wants. `None` sweeps the whole backlog.
///
/// Task selection, the permissive metadata parse and the git memo are
/// [`crate::task_rows`], shared with PTODO's ζ inverse lane. What is this
/// lane's own is the quantifier and the finding kind, and both diverge from ζ
/// deliberately:
///
/// 1. Iteration is per ROW with an all-paths-absent quantifier, not per path,
///    because a row's `paths` are ONE ANY-match pathspec (see the module doc).
///    ζ's per-entry iteration is correct only because each `metadata.files`
///    entry stands alone.
/// 2. The finding kind comes from the row's `expect` polarity, not from
///    rename-vs-delete; that axis changes only the repair hint, which is
///    carried as evidence.
///
/// A row all of whose paths are absent but NONE of which git has ever seen is
/// presumed to name files the task will CREATE, and passes — the same
/// load-bearing arm [`PathHistory::resolve`] documents, and what keeps healthy
/// post-state rows quiet.
///
/// Findings are sorted by (task id as integer, check name) for determinism.
/// Fail-soft on DB errors, propagated as `Err` for the caller to degrade.
// G-allow: test-facing thin pub fn — its callers are the tests/pdcheck.rs integration test binary (a SEPARATE crate, so `pub(crate)` would break it) and `check` in this module. Mirrors ptodo::resolve_inverse's pub-for-integration-test pattern.
pub fn resolve_delivered_check_paths(
    conn: &rusqlite::Connection,
    git: &dyn GitOps,
    tracked: &HashSet<String>,
    target_task_id: Option<&str>,
) -> rusqlite::Result<Vec<Finding>> {
    let mut out: Vec<Finding> = Vec::new();
    let mut history = PathHistory::default();

    for (id, metadata) in non_terminal_master_tasks(conn)? {
        // Optional single-task narrowing (mirrors p5_phantom_done::check_with_target).
        if let Some(target) = target_task_id
            && id.to_string() != target
        {
            continue;
        }

        let rows: Vec<DeliveredCheckRow> = metadata_array(&metadata, "delivered_checks")
            .iter()
            .filter_map(DeliveredCheckRow::from_json)
            .collect();

        for row in rows {
            let Some(verdict) = classify_row(&row, tracked) else {
                continue;
            };

            // Every path in the row is absent. Ask git which of them ever
            // existed: a path with no history is presumed to-be-created, and a
            // row of only such paths is not a finding.
            let dead: Vec<DeadPath> = row
                .paths
                .iter()
                .filter_map(|path| {
                    let (commit, rename_target) = history.resolve(git, tracked, path)?;
                    Some(DeadPath { path: path.clone(), commit, rename_target })
                })
                .collect();
            if dead.is_empty() {
                continue;
            }

            out.push(build_finding(id, &row, verdict, &dead));
        }
    }

    out.sort_by(|a, b| {
        let id_a = a.task_id.parse::<i64>().unwrap_or(i64::MAX);
        let id_b = b.task_id.parse::<i64>().unwrap_or(i64::MAX);
        id_a.cmp(&id_b).then_with(|| check_name_of(a).cmp(&check_name_of(b)))
    });
    Ok(out)
}

/// This lane's sort key within a task: the flagged row's `name`, read back out
/// of the evidence that carries it.
fn check_name_of(finding: &Finding) -> String {
    finding
        .evidence
        .iter()
        .find_map(|e| match e {
            EvidenceRef::DeliveredCheck { check_name, .. } => Some(check_name.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// One finding per flagged row. The repair hint names the first dead path that
/// was renamed somewhere still tracked, since that is the one a fixer can act
/// on; evidence carries every dead path's commit and target.
fn build_finding(id: i64, row: &DeliveredCheckRow, verdict: Verdict, dead: &[DeadPath]) -> Finding {
    // Name the source path in the hint too: with several dead paths in one row
    // a bare target leaves the fixer guessing which path it replaces.
    let hint = match dead.iter().find(|d| d.rename_target.is_some()) {
        Some(DeadPath { path, rename_target: Some(target), commit }) => {
            format!(" — repoint '{path}' to '{target}' (renamed in {})", commit.sha)
        }
        _ => format!(
            " — '{}' last touched in {}",
            dead[0].path, dead[0].commit.sha
        ),
    };

    let mut evidence = vec![EvidenceRef::DeliveredCheck {
        check_name: row.name.clone(),
        paths: row.paths.clone(),
    }];
    for entry in dead {
        if let Some(target) = &entry.rename_target {
            evidence.push(EvidenceRef::File { path: target.clone() });
        }
        if !evidence
            .iter()
            .any(|e| matches!(e, EvidenceRef::Commit { sha, .. } if *sha == entry.commit.sha))
        {
            evidence.push(EvidenceRef::Commit {
                sha: entry.commit.sha.clone(),
                subject: entry.commit.subject.clone(),
            });
        }
    }

    Finding {
        pattern: Pattern::PDeliveredCheckPath,
        severity: verdict.severity(),
        task_id: id.to_string(),
        summary: format!(
            "{kind}: task #{id} check '{name}' (expect={polarity}, pattern '{pattern}') names no tracked path: {paths}{hint}",
            kind = verdict.kind(),
            name = row.name,
            polarity = row.expect.as_deref().unwrap_or_default(),
            pattern = row.pattern.as_deref().unwrap_or_default(),
            paths = row.paths.join(", "),
        ),
        evidence,
    }
}

/// [`crate::AuditContext`] entry point.
///
/// Degrades fail-soft: a missing or unreadable `.taskmaster/tasks/tasks.db` is
/// an absent OPTIONAL substrate, so it yields zero findings behind one stderr
/// breadcrumb naming the resolved path rather than an error exit. The exit
/// class is untouched — 125 is reserved for genuine arg/IO misconfig.
pub fn check(ctx: &crate::AuditContext) -> Vec<Finding> {
    let tracked: HashSet<String> = ctx.git.ls_files().into_iter().collect();
    let db_path = crate::ptodo::tasks_db_path(&ctx.project_root);
    match crate::ptodo::open_tasks_db(&db_path).and_then(|conn| {
        resolve_delivered_check_paths(&conn, ctx.git, &tracked, ctx.target_task_id.as_deref())
    }) {
        Ok(findings) => findings,
        Err(e) => {
            // Name the CAUSE, not a presumed one: this `Err` also covers a DB
            // that opened fine and then failed to query (schema drift, a
            // corrupt page). Blaming an absent file for that would send the
            // reader looking for a path that is sitting right there.
            let cause = if db_path.exists() { "query failed" } else { "absent" };
            eprintln!(
                "reify-audit: PDCHECK delivered_checks dead-path lane skipped — tasks.db {cause} at '{}': {e}; this is NOT a clean bill of health",
                db_path.display()
            );
            Vec::new()
        }
    }
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

    /// `delivered_checks[].paths` is a git PATHSPEC list, and git resolves
    /// magic this lane's membership test cannot: `crates/reify-ir/src/*.rs`
    /// equals no tracked entry and prefixes none, yet greps a live file set.
    /// Reading that `false` as "absent" would manufacture a HIGH finding
    /// against a perfectly satisfiable row — the lane's only false-positive
    /// class — so a magic pathspec silences the whole row instead.
    #[test]
    fn pathspec_magic_silences_the_row_under_either_polarity() {
        for pathspec in [
            "crates/reify-ir/src/*.rs",
            "crates/reify-ir/src/arg_acceptance.?s",
            "crates/reify-ir/src/arg_acceptance.[rs]s",
            ":(exclude)crates/reify-eval/src/arg_acceptance.rs",
            ":!crates/reify-eval/src/arg_acceptance.rs",
            ":/crates",
        ] {
            for expect in ["present", "absent"] {
                let r = row(json!({
                    "name": "angle-spec-absent-today",
                    "kind": "grep",
                    "expect": expect,
                    "pattern": "pub fn angle_spec",
                    "paths": [pathspec],
                }));
                assert_eq!(
                    classify_row(&r, &tracked(&[LIVE])),
                    None,
                    "pathspec magic '{pathspec}' is unclassifiable here (expect: {expect})"
                );
            }
        }
    }

    /// One magic path is enough: the row is a SINGLE ANY-match grep, so a
    /// pathspec this lane cannot resolve leaves the whole row unreadable —
    /// exactly as one live path does.
    #[test]
    fn one_magic_path_silences_a_row_whose_other_paths_are_dead() {
        let r = row(json!({
            "name": "angle-spec-absent-today",
            "kind": "grep",
            "expect": "present",
            "pattern": "pub fn angle_spec",
            "paths": [DEAD, "crates/reify-ir/src/*.rs"],
        }));
        assert_eq!(classify_row(&r, &tracked(&[LIVE])), None);
    }

    /// The guard must not swallow ordinary paths: a plain path carrying none
    /// of the magic characters still classifies, or the whole lane goes quiet.
    #[test]
    fn ordinary_paths_are_not_mistaken_for_pathspec_magic() {
        for path in [DEAD, LIVE, "crates/reify-eval/tests/", "a-b_c.d/e.rs"] {
            assert!(!super::is_pathspec_magic(path), "'{path}' carries no magic");
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
