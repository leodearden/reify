//! POLDCORE: the old-core hold. Work on the old evaluation core is held
//! behind a holder task while a new core is built beside it. A task is held by
//! a direct dependency edge on the holder, or released by
//! `metadata.old_core_release`. This module reports the open tasks the hold
//! missed, and the holder's own state.
//!
//! # Kinds
//!
//! Each kind is a stable summary prefix:
//!
//! - `hold-missed:` (High): an open task, not the holder, with no direct edge
//!   on the holder and no release, where at least one and at least half of
//!   its non-test `metadata.files` are held.
//! - `hold-missed-registration-only:` (Low): the same, when every held file is
//!   a `registration_only` entry (lib.rs), which any task adding a module
//!   lists.
//! - `holder-state:` (High): at most one per run, naming every defect. The
//!   holder is absent from the corpus, not `pending`, has no `deferred_watch`,
//!   or has `trigger_met`; each one lets the hold release.
//! - `hold-list-invalid:` (High): the hold list cannot be read or breaks an
//!   invariant. It is then the run's only finding, so a broken list never
//!   reads as clean.
//!
//! An edge removed without a release is not a kind of its own: the store
//! records a hold only as the edge, so such a task is indistinguishable from
//! one never held, and it is reported only when it meets the rule.
//!
//! # Inputs
//!
//! Tasks come from `ctx.task_metadata()` (the fused-memory live loader, or
//! `--tasks-file`), never the task DB. The list comes from
//! [`HOLD_LIST_PATH`] through `ctx.read_relative`. It never calls git or
//! jcodemunch.
//!
//! # Why opt-in
//!
//! Task state lives only with the main checkout's fused-memory, so a task
//! worktree's verify gate cannot see it. The CLI's exit code is the High
//! count, and each High awaits a ruling. So it runs only under
//! `--pattern POLDCORE`.
//!
//! What the hold is for, and how to hold or release a task:
//! `docs/notes/old-evaluation-core-hold.md`.

use crate::old_core_hold::{HOLD_LIST_PATH, HeldShare, HoldList};
use crate::task_rows::is_terminal_status;
use crate::{AuditContext, EvidenceRef, Finding, Pattern, Severity, TaskMetadata};
use std::collections::HashMap;
use std::fmt;

const HOLD_MISSED: &str = "hold-missed";
const HOLD_MISSED_REGISTRATION_ONLY: &str = "hold-missed-registration-only";
const HOLDER_STATE: &str = "holder-state";
const HOLD_LIST_INVALID: &str = "hold-list-invalid";
const HOLD_NOTE: &str = "docs/notes/old-evaluation-core-hold.md";

/// The list fault alone, or the holder's state followed by every missed task
/// in numeric id order.
pub fn check(ctx: &AuditContext) -> Vec<Finding> {
    let list = match load_hold_list(ctx) {
        Ok(list) => list,
        Err(fault) => return vec![fault],
    };
    let tasks = ctx.task_metadata();
    let mut missed: Vec<Finding> = tasks
        .values()
        .filter_map(|task| missed_finding(&list, task))
        .collect();
    missed.sort_by_key(|f| {
        (
            f.task_id.parse::<u64>().unwrap_or(u64::MAX),
            f.task_id.clone(),
        )
    });
    holder_finding(&list, tasks)
        .into_iter()
        .chain(missed)
        .collect()
}

fn load_hold_list(ctx: &AuditContext) -> Result<HoldList, Finding> {
    let text = ctx.read_relative(HOLD_LIST_PATH).ok_or_else(|| {
        list_fault(format!(
            "{HOLD_LIST_PATH} cannot be read (absent, unreadable or not UTF-8)"
        ))
    })?;
    HoldList::parse(&text).map_err(|e| list_fault(format!("{HOLD_LIST_PATH}: {e}")))
}

fn list_fault(reason: String) -> Finding {
    Finding {
        pattern: Pattern::POldCoreHold,
        severity: Severity::High,
        task_id: HOLD_LIST_PATH.to_string(),
        summary: format!(
            "{HOLD_LIST_INVALID}: {reason}; POLDCORE checked no task this run, \
             so the absence of other findings is not a clean result"
        ),
        evidence: vec![list_evidence()],
    }
}

fn list_evidence() -> EvidenceRef {
    EvidenceRef::File {
        path: HOLD_LIST_PATH.to_string(),
    }
}

/// A way the holder fails to hold.
enum HolderDefect {
    Absent,
    NotPending(String),
    NoDeferredWatch,
    TriggerMet,
}

impl fmt::Display for HolderDefect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HolderDefect::Absent => write!(f, "is absent from the task corpus"),
            HolderDefect::NotPending(status) => {
                write!(f, "has status `{status}`, not `pending`")
            }
            HolderDefect::NoDeferredWatch => write!(f, "has no truthy metadata.deferred_watch"),
            HolderDefect::TriggerMet => write!(f, "has a truthy metadata.trigger_met"),
        }
    }
}

fn holder_defects(holder: Option<&TaskMetadata>) -> Vec<HolderDefect> {
    let Some(holder) = holder else {
        return vec![HolderDefect::Absent];
    };
    let mut defects = Vec::new();
    if holder.status != "pending" {
        defects.push(HolderDefect::NotPending(holder.status.clone()));
    }
    if !holder.has_deferred_watch {
        defects.push(HolderDefect::NoDeferredWatch);
    }
    if holder.trigger_met {
        defects.push(HolderDefect::TriggerMet);
    }
    defects
}

fn holder_finding(list: &HoldList, tasks: &HashMap<String, TaskMetadata>) -> Option<Finding> {
    let holder = list.holder_id();
    let defects = holder_defects(tasks.get(holder));
    if defects.is_empty() {
        return None;
    }
    let defects = defects
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    Some(Finding {
        pattern: Pattern::POldCoreHold,
        severity: Severity::High,
        task_id: holder.to_string(),
        summary: format!(
            "{HOLDER_STATE}: holder #{holder} {defects}; while it stands the hold can \
             release every held task. Restore it per {HOLD_NOTE}"
        ),
        evidence: vec![list_evidence()],
    })
}

/// An open task, not the holder, that is neither held by a direct edge nor
/// released.
fn outside_the_hold(list: &HoldList, task: &TaskMetadata) -> bool {
    let holder = list.holder_id();
    !is_terminal_status(&task.status)
        && task.task_id != holder
        && !task.has_old_core_release
        && !task.dependencies.iter().any(|d| d == holder)
}

fn missed_finding(list: &HoldList, task: &TaskMetadata) -> Option<Finding> {
    if !outside_the_hold(list, task) {
        return None;
    }
    let HeldShare { held, non_test } = list.held_share_of(&task.files);
    if held.is_empty() || 2 * held.len() < non_test {
        return None;
    }
    let (kind, severity) = if held.iter().all(|f| list.is_registration_only(f)) {
        (HOLD_MISSED_REGISTRATION_ONLY, Severity::Low)
    } else {
        (HOLD_MISSED, Severity::High)
    };
    let id = &task.task_id;
    let holder = list.holder_id();
    Some(Finding {
        pattern: Pattern::POldCoreHold,
        severity,
        task_id: id.clone(),
        summary: format!(
            "{kind}: #{id} '{title}' lists held old-core files {files} ({k} of {n} \
             non-test files) with no dependency on holder #{holder} and no \
             metadata.old_core_release; hold it with add_dependency({id}, {holder}), \
             or release it per {HOLD_NOTE}",
            title = task.title,
            files = held.join(", "),
            k = held.len(),
            n = non_test,
        ),
        evidence: vec![
            EvidenceRef::MetadataFiles {
                entries: held.iter().map(|f| f.to_string()).collect(),
            },
            list_evidence(),
        ],
    })
}
