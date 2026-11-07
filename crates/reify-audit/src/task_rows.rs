//! Shared scaffolding for the detector lanes that read the task DB.
//!
//! Two lanes walk `.taskmaster/tasks/tasks.db` asking the same question of
//! different metadata fields — PTODO's ζ inverse lane over `metadata.files`,
//! and [`crate::pdcheck`] over `metadata.delivered_checks[].paths`. What they
//! share is everything except the question: which tasks to consider, how
//! permissively to read their metadata, what "this path still exists" means,
//! and how to ask git about one that does not without re-spawning a
//! subprocess per citation.
//!
//! That shared half lives here so a fix to the terminal-status set, the memo
//! keying, or the never-existed arm is made ONCE. Each lane keeps only its own
//! quantifier and finding construction.

use crate::{GitCommit, GitOps};
use std::collections::{HashMap, HashSet};

/// §8.4 terminal statuses: a cite resolving to one of these is "dead" and
/// orphans its marker. Every other present status (pending / in-progress /
/// blocked / deferred) is nominally live — but see `metadata_do_not_complete`:
/// a non-terminal task carrying `do_not_complete == true` is classified as
/// `parked-on-anchor` (Medium) rather than live (task ι, #4644). η flips
/// `orphaned` to High; β keeps all other liveness kinds Medium.
///
/// Both DB lanes apply this same skip: only a task that can still land can
/// block a dependent.
pub(crate) fn is_terminal_status(status: &str) -> bool {
    status == "done" || status == "cancelled"
}

/// Membership test shared by both lanes: `true` when `path` (trailing-slash-
/// tolerant) is "present in the tracked set" — i.e. it equals a tracked file
/// OR is a directory prefix of some tracked file (a tracked file starts with
/// `path + "/"`). Strips at most one trailing `/` before the checks.
///
/// This guard suppresses the critical FP class where a cited path names a
/// DIRECTORY that still exists (e.g. `crates/reify-audit/tests`): a directory
/// is never a member of the `git ls-files` set, yet `git log -1 -- <dir>`
/// returns non-empty — without this guard, every directory citation would
/// produce a false-positive finding.
///
/// It models exact-match and directory-prefix membership ONLY. A git pathspec
/// carrying MAGIC (a leading `:`, an fnmatch wildcard) resolves for git and
/// not here; a caller admitting such pathspecs must screen them out itself
/// rather than read a `false` as "absent" (see `pdcheck::is_pathspec_magic`).
pub(crate) fn path_present_in_tracked(path: &str, tracked: &HashSet<String>) -> bool {
    // Strip at most one trailing slash for both exact-match and prefix checks.
    let path = path.trim_end_matches('/');
    if tracked.contains(path) {
        return true;
    }
    // Directory-prefix membership: some tracked file lives under `path/`.
    // O(n) scan over the tracked set — acceptable for current backlog sizes
    // because most cited paths hit the O(1) exact-match branch above and only
    // genuinely absent paths reach here. If the tracked set grows very large
    // (tens of thousands of files), consider a sorted Vec<String> +
    // `partition_point`-based prefix search to reduce this to O(log n).
    let prefix = format!("{}/", path);
    tracked.iter().any(|f| f.starts_with(&prefix))
}

/// Every non-terminal `master` task, paired with its parsed metadata.
///
/// Metadata is read permissively because it is written by another repo: NULL
/// or unparseable JSON lands as [`serde_json::Value::Null`], on which every
/// [`metadata_array`] lookup is empty. Nothing here may fail a task — let
/// alone panic — over a shape it does not recognise.
///
/// Fail-soft on DB errors, propagated as `Err` for the caller to degrade.
pub(crate) fn non_terminal_master_tasks(
    conn: &rusqlite::Connection,
) -> rusqlite::Result<Vec<(i64, serde_json::Value)>> {
    let mut stmt = conn.prepare("SELECT id, status, metadata FROM tasks WHERE tag = 'master'")?;
    let rows: Vec<(i64, String, Option<String>)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;

    Ok(rows
        .into_iter()
        .filter(|(_, status, _)| !is_terminal_status(status))
        .map(|(id, _, metadata)| {
            let parsed = metadata
                .and_then(|m| serde_json::from_str::<serde_json::Value>(&m).ok())
                .unwrap_or(serde_json::Value::Null);
            (id, parsed)
        })
        .collect())
}

/// A task-metadata array field. A missing key, a null, and a non-array value
/// are all the empty slice — see [`non_terminal_master_tasks`] on why every
/// shape must be inert rather than fatal.
pub(crate) fn metadata_array<'a>(
    metadata: &'a serde_json::Value,
    key: &str,
) -> &'a [serde_json::Value] {
    metadata
        .get(key)
        .and_then(|v| v.as_array())
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// Per-run memo over what git knows about paths a lane has found absent.
///
/// Both caches exist for a measured reason: one relocated file is routinely
/// cited by several related tasks (the live case was 2 renamed paths cited by
/// 6 tasks), and each miss is a subprocess spawn.
#[derive(Default)]
pub(crate) struct PathHistory {
    last_commit: HashMap<String, Option<GitCommit>>,
    /// Keyed on (path, sha) rather than the path alone: within a run the sha
    /// IS a pure function of the path (it comes from `last_commit`), but the
    /// tuple key is correct without depending on that invariant, and matches
    /// the `HashMap<(String, String), _>` shape the `GitOps` mock uses.
    rename_target: HashMap<(String, String), Option<String>>,
}

impl PathHistory {
    /// What git knows about an absent `path`: the commit that last touched it,
    /// and the path it was renamed to — but only when that target is ITSELF
    /// still tracked, so no lane ever advertises a path the reader cannot open.
    ///
    /// `None` is the load-bearing arm: git has never seen this path, so it is
    /// presumed to-be-created and is not a finding in either lane. Any git
    /// error degrades into that same silence rather than a false positive.
    pub(crate) fn resolve(
        &mut self,
        git: &dyn GitOps,
        tracked: &HashSet<String>,
        path: &str,
    ) -> Option<(GitCommit, Option<String>)> {
        let commit = self
            .last_commit
            .entry(path.to_string())
            .or_insert_with(|| git.last_commit_for_path(path))
            .clone()?;
        let rename_target = self
            .rename_target
            .entry((path.to_string(), commit.sha.clone()))
            .or_insert_with(|| git.rename_target_for_path(path, &commit.sha))
            .clone()
            // `path_present_in_tracked`, not a bare `contains`, so this test
            // cannot drift from the one that found the path absent.
            .filter(|target| path_present_in_tracked(target, tracked));
        Some((commit, rename_target))
    }
}
