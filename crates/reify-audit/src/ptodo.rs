//! PTODO — TODO-tracking-invariant detector (structural lane α + liveness lane β).
//!
//! Scans the working tree for TODO-family markers and classifies each through
//! two lanes, all emitting Medium-severity findings:
//!
//! - **Structural lane (α)** — markers not backed by a canonical `#NNNN` task
//!   citation: `untracked` / `malformed-cite` / `phantom-tracking` /
//!   `bare-ignore`. The grammar lives in pure `&str -> result` functions
//!   (mirroring P2's `line_matches_stub`/`scan_file_added_lines` split, no
//!   `regex` dependency per design §12).
//! - **Liveness lane (β)** — every canonical `#NNNN` cite the structural lane
//!   treats as "tracked" is resolved against `.taskmaster/tasks/tasks.db`
//!   (opened read-only): a cite whose status is terminal (done / cancelled) →
//!   `orphaned`; a cite absent from the DB → `unknown-id`. Per §8.2 one live
//!   cite suffices to track a marker. The lane degrades fail-soft (§6.7): a
//!   missing/unreadable DB is skipped with a single stderr breadcrumb while the
//!   structural lane still runs in full.
//!
//! A single precedence-correct `scan_file` pass feeds both lanes so they
//! never drift. Only file enumeration (`GitOps::ls_files`), content reads
//! (`std::fs::read_to_string`), and the read-only task-DB open touch IO, inside
//! [`check`].
//!
//! Reference: `docs/prds/reify-audit-ptodo-detector.md` §8 (normative grammar),
//! §6.7 (liveness degradation contract).

use crate::{AuditContext, EvidenceRef, Finding, GitCommit, Pattern, Severity};
use reify_test_support::ignore_hygiene::extract_ignore_reason;
use rusqlite::OptionalExtension;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// -----------------------------------------------------------------------
// §8.1 marker recognition (pure, hand-rolled — no `regex` dep per design §12)
// -----------------------------------------------------------------------

/// `true` when `b` is an ASCII word byte (`[A-Za-z0-9_]`) — the alphabet for
/// the hand-rolled `\b` word-boundary checks in [`find_comment_marker`].
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `char`-level analogue of [`is_word_byte`] for the `\b` left-boundary check
/// in [`has_malformed_cite`] (which scans `char`s to recognise Greek cites).
fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// §8.1 comment markers — canonical regex `\b(TODO|FIXME|HACK)\b\s*[(:]`.
///
/// Case-sensitive uppercase only: lowercase prose ("todo: someday") does not
/// fire (design decision — cuts false positives). The keyword must be a whole
/// word (non-word byte / line edge on both sides, so `XTODO`/`TODONE` miss),
/// optionally followed by whitespace, then `(` or `:`. Returns the matched
/// keyword, or `None`.
fn find_comment_marker(line: &str) -> Option<&'static str> {
    let bytes = line.as_bytes();
    for kw in ["TODO", "FIXME", "HACK"] {
        let klen = kw.len();
        let mut start = 0;
        while let Some(rel) = line[start..].find(kw) {
            let idx = start + rel;
            let after = idx + klen;
            let left_ok = idx == 0 || !is_word_byte(bytes[idx - 1]);
            let right_ok = after >= bytes.len() || !is_word_byte(bytes[after]);
            if left_ok && right_ok {
                let mut j = after;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < bytes.len() && (bytes[j] == b'(' || bytes[j] == b':') {
                    return Some(kw);
                }
            }
            start = idx + 1;
        }
    }
    None
}

/// §8.1 Rust stub macros: `todo!(` / `unimplemented!(`. Pure substring scan;
/// the `.rs`-only gating lives in [`scan_file`]. A line whose trimmed start
/// is a `//` comment (`//`, `///`, `//!`) is prose, not a real stub — a
/// commented-out or doc-comment mention (`// todo!() example`) does not fire
/// (mirrors the doc-comment skip in [`ignore_attr`]).
fn find_macro_stub(line: &str) -> bool {
    if line.trim_start().starts_with("//") {
        return false;
    }
    line.contains("todo!(") || line.contains("unimplemented!(")
}

/// The two §8.1 `#[ignore]` shapes the structural lane distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IgnoreForm {
    /// `#[ignore]` — no reason string (α emits `bare-ignore`).
    Bare,
    /// `#[ignore = "..."]` — carries a reason (α defers the reason policy to γ).
    WithReason,
}

/// §8.1 ignore attributes (`.rs` only — gating in [`scan_file`]): a trimmed
/// line that starts with `#[ignore`. `///`/`//!` doc-comment prose mentioning
/// the attribute does not fire. `]` immediately after → `Bare`; `=` →
/// `WithReason`.
fn ignore_attr(line: &str) -> Option<IgnoreForm> {
    let t = line.trim_start();
    if t.starts_with("///") || t.starts_with("//!") {
        return None;
    }
    let rest = t.strip_prefix("#[ignore")?.trim_start();
    if rest.starts_with(']') {
        Some(IgnoreForm::Bare)
    } else if rest.starts_with('=') {
        Some(IgnoreForm::WithReason)
    } else {
        None
    }
}

// -----------------------------------------------------------------------
// §8.2 citation resolution (canonical vs malformed)
// -----------------------------------------------------------------------

/// §8.2 canonical citation: a `#` immediately followed by a run of 1..=5 ASCII
/// digits whose run length is ≤5 (the char after the run is not a digit, so a
/// 6-digit number is not matched on its 5-digit prefix) AND whose value is ≥1.
/// An all-zero run (`#0`, `#00`) is rejected — task ids start at 1, so a `#0`
/// cite is not canonical and falls through to the structural `untracked`
/// classification (mirrors the ≥1 guard in [`extract_cites`]).
fn has_canonical_cite(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            let run = j - (i + 1);
            // ≥1 guard: the run must carry a non-zero digit (`#0`/`#00` → 0 → not
            // a valid task id). `#007` (= 7) is still canonical.
            if (1..=5).contains(&run) && bytes[i + 1..j].iter().any(|&b| b != b'0') {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// §8.2 cite extraction (β liveness lane): every canonical `#NNNN` id on the
/// line, in source order. Mirrors [`has_canonical_cite`]'s `#`+digit-run scan
/// but parses each 1..=5-digit run to `u32` (runs of length 0 or >5 are
/// skipped, so `#abc`, a bare `#`, and a 6-digit `#123456` yield nothing —
/// consistent with the canonical-cite recogniser). The id-0 case (`#0`, `#00`)
/// is also skipped — task ids start at 1, so a `#0` cite is not a valid id and
/// is dropped here (keeping it lock-step with [`has_canonical_cite`]'s ≥1 guard,
/// so `#0` classifies structurally as `untracked` rather than spuriously
/// `unknown-id`).
fn extract_cites(line: &str) -> Vec<u32> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            let run = j - (i + 1);
            if (1..=5).contains(&run) {
                // `line[i + 1..j]` is a 1..=5-digit ASCII run; it always fits
                // in u32 (max 99999), so the parse cannot fail. Skip id 0 (`#0`,
                // `#00`) — task ids start at 1, so it is not a valid cite.
                if let Ok(id) = line[i + 1..j].parse::<u32>()
                    && id >= 1
                {
                    out.push(id);
                }
                i = j; // skip past the consumed digit run
                continue;
            }
        }
        i += 1;
    }
    out
}

/// `true` when `c` is a Greek-block letter (U+0370..=U+03FF) — the banned
/// Greek-cite alphabet (`task δ`, `task α`).
fn is_greek(c: char) -> bool {
    ('\u{0370}'..='\u{03FF}').contains(&c)
}

/// §8.2/§6.4 malformed citation: the case-insensitive token `task` immediately
/// followed — after an optional single space — by a Greek letter, OR
/// `task-`/`task_`/`task `+ ASCII digit (PRD-relative / legacy forms). Banned
/// from day one; δ migrates valid cites to canonical `#NNNN`.
fn has_malformed_cite(line: &str) -> bool {
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i + 4 <= n {
        let is_task = chars[i].eq_ignore_ascii_case(&'t')
            && chars[i + 1].eq_ignore_ascii_case(&'a')
            && chars[i + 2].eq_ignore_ascii_case(&'s')
            && chars[i + 3].eq_ignore_ascii_case(&'k');
        // Require a left word boundary so an embedded `task` (e.g. the one
        // inside `multitask 5`) is not misread as a malformed cite — mirrors
        // the `\b` logic in `find_comment_marker`.
        let left_ok = i == 0 || !is_word_char(chars[i - 1]);
        if is_task && left_ok {
            let after = i + 4;
            if after < n {
                let c = chars[after];
                // Greek immediately after `task`.
                if is_greek(c) {
                    return true;
                }
                // Digit form: `task` + (`-` | `_` | ` `) + ASCII digit.
                if (c == '-' || c == '_' || c == ' ')
                    && after + 1 < n
                    && chars[after + 1].is_ascii_digit()
                {
                    return true;
                }
                // Greek after a single space: `task δ`.
                if c == ' ' && after + 1 < n && is_greek(chars[after + 1]) {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

// -----------------------------------------------------------------------
// §8.3 phantom tracking / §6.8 inline escape, allowlist, swept extensions
// -----------------------------------------------------------------------

/// §8.3 phantom-tracking phrases — prose that *claims* a TODO is tracked
/// elsewhere without backing it with a canonical `#NNNN` cite. Matched
/// case-insensitively (lowercase copy) as substrings.
const PHANTOM_PHRASES: &[&str] = &[
    "tracked separately",
    "tracked as a follow-up",
    "tracked in project memory",
    "follow-up task will",
];

/// §8.3 phantom-tracking detection: `true` when the line contains any of the
/// [`PHANTOM_PHRASES`] (case-insensitive). The no-canonical-cite precondition
/// is applied by the caller ([`scan_file`]).
fn phantom_phrase(line: &str) -> bool {
    let lower = line.to_lowercase();
    PHANTOM_PHRASES.iter().any(|p| lower.contains(p))
}

/// §8.3 γ blocker-prose needles — matched case-insensitively (against a
/// lowercased copy of the reason), except `RED:` which is matched
/// case-sensitively against the original to avoid the `required:` false
/// positive (the substring `red:` appears in `required:` when lowercased).
///
/// Trailing spaces on `until ` and `once ` are part of the §8.3 grammar and
/// provide a crude word boundary (so `until` at end-of-string does not match).
const BLOCKER_PROSE: &[&str] = &["pending", "not yet", "until ", "once ", "blocked"];

/// §8.3 γ: `true` when `reason` contains a blocker-prose needle.
///
/// The check is applied to the EXTRACTED reason, not the whole `#[ignore]`
/// line. Five tokens are matched case-insensitively; `RED:` is matched
/// case-sensitively to guard against `required:` false positives.
fn has_blocker_prose(reason: &str) -> bool {
    let lower = reason.to_lowercase();
    if BLOCKER_PROSE.iter().any(|n| lower.contains(n)) {
        return true;
    }
    // `RED:` case-sensitive — `required:` must not match.
    reason.contains("RED:")
}

/// §6.8 inline escape: a line carrying the literal `ptodo:allow` opts out of
/// the whole sweep for that line (an intentional, reviewed marker).
fn line_escaped(line: &str) -> bool {
    line.contains("ptodo:allow")
}

/// §6.8 allowlist path prefixes — paths starting with any entry are exempt
/// from the sweep so the tool never flags its own machinery or test data.
const ALLOWLIST_PREFIXES: &[&str] = &[
    // The detector's own crate: its pattern string-literals (`TODO`/`FIXME`/
    // `HACK`, `task δ`, the phantom phrases, …) and the committed fixtures
    // under `tests/fixtures/ptodo/` would otherwise self-match.
    "crates/reify-audit/",
    // The `#[ignore]`-reason extraction tool: carries `#[ignore]` markers and
    // reason strings as the data it operates on.
    "crates/reify-test-support/src/ignore_hygiene.rs",
    // … and that tool's tests, which embed `#[ignore]` attributes as fixtures.
    "crates/reify-test-support/tests/ignore_reason_hygiene.rs",
];

/// §6.8 allowlist check: `true` when `path` (root-relative) starts with any
/// [`ALLOWLIST_PREFIXES`] entry.
fn is_allowlisted(path: &str) -> bool {
    ALLOWLIST_PREFIXES.iter().any(|prefix| path.starts_with(prefix))
}

/// §6.8 swept extensions — the exact set the structural lane scans:
/// `.rs .ri .sh .py .ts .tsx .js`. Non-code/config files (`.md`, `.toml`,
/// `.yaml`, `.json`, …) carry prose, not tracked-work markers, and are skipped
/// (PRD §13 Q1 defers `.toml`/`.yml`/`.yaml` to θ).
fn is_swept_ext(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".rs")
        || lower.ends_with(".ri")
        || lower.ends_with(".sh")
        || lower.ends_with(".py")
        || lower.ends_with(".ts")
        || lower.ends_with(".tsx")
        || lower.ends_with(".js")
}

// -----------------------------------------------------------------------
// §8.3 per-file classification
// -----------------------------------------------------------------------

/// The four structural-lane finding kinds α emits (all Medium severity). The
/// §8.3 `kind` token is carried as a stable summary prefix under the single
/// [`Pattern::PTodo`](crate::Pattern::PTodo) variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A TODO-family marker with no task citation at all.
    Untracked,
    /// A marker citing a task in a banned form (Greek / PRD-relative / legacy).
    MalformedCite,
    /// Prose claiming the work is tracked elsewhere, with no canonical cite.
    PhantomTracking,
    /// A bare `#[ignore]` attribute (no reason string).
    BareIgnore,
}

impl Kind {
    /// The §8.3 kind token, used as the finding summary prefix.
    fn as_str(self) -> &'static str {
        match self {
            Kind::Untracked => "untracked",
            Kind::MalformedCite => "malformed-cite",
            Kind::PhantomTracking => "phantom-tracking",
            Kind::BareIgnore => "bare-ignore",
        }
    }
}

/// The unified per-line classification produced by [`scan_file`]. A given line
/// is either *structurally* offending (no canonical cite → α's domain) or
/// *cited* (a canonical `#NNNN` marker → β's liveness domain). At most one
/// variant per line; lines matching neither produce no entry.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LineClass {
    /// A structural finding kind (α) — constructed by [`scan_file`].
    Structural(Kind),
    /// A tracked marker carrying one or more canonical `#NNNN` cites (β) — the
    /// liveness lane resolves these ids against the task DB.
    Cited(Vec<u32>),
}

/// §8 per-file scan: walk `content` line-by-line and return one `(line_no,
/// class, marker_text)` entry per offending OR tracked line (1-based line
/// numbers, `marker_text` is the trimmed line). `is_rust` gates the `.rs`-only
/// macro and `#[ignore]` rules. This is the single precedence-correct pass
/// shared by the structural lane and the liveness lane (both driven from
/// [`check`]) so the two never drift.
///
/// Precedence per line (first match wins; at most one entry per line):
/// 1. `ptodo:allow` inline escape → the line is skipped entirely (§6.8).
/// 2. `#[ignore]` (`.rs`): bare → `Structural(BareIgnore)`; reason-bearing →
///    γ reason policy: extract reason via [`extract_ignore_reason`]; if it
///    contains a canonical `#NNNN` cite → `Cited(ids)` (step-8; β liveness);
///    else if it has blocker-prose → `Structural(Untracked)`; else (operational)
///    → no entry. Checked before comment markers so a reason string is not
///    misread as a marker.
/// 3. comment marker (all exts): canonical `#NNNN` → `Cited(on-line cites)`
///    (tracked → β liveness-checks); malformed cite → `Structural(MalformedCite)`;
///    else `Structural(Untracked)`.
/// 4. stub macro (`.rs`): a canonical cite on this line OR the line directly
///    above → `Cited(this-line ∪ above-line cites)` (above-line lookback for the
///    `// #NNNN` \ `todo!()` convention); else `Structural(Untracked)`.
/// 5. phantom phrase with no canonical cite → `Structural(PhantomTracking)`.
fn scan_file(content: &str, is_rust: bool) -> Vec<(usize, LineClass, String)> {
    let mut out = Vec::new();
    let mut prev: Option<&str> = None;
    for (i, line) in content.lines().enumerate() {
        let line_no = i + 1;

        // (1) inline escape — opt this line out of the whole sweep.
        if line_escaped(line) {
            prev = Some(line);
            continue;
        }

        let has_canon = has_canonical_cite(line);

        if is_rust && let Some(form) = ignore_attr(line) {
            // (2) #[ignore] (.rs only). γ reason policy (cite-first, §8.3):
            //   bare → Structural(BareIgnore);
            //   reason-bearing: extract reason;
            //     if it has a canonical cite → Cited(ids) (β liveness);
            //     else if it has blocker-prose → Structural(Untracked);
            //     else (operational) → no entry.
            match form {
                IgnoreForm::Bare => {
                    out.push((line_no, LineClass::Structural(Kind::BareIgnore), line.trim().to_string()));
                }
                IgnoreForm::WithReason => {
                    if let Some(reason) = extract_ignore_reason(line) {
                        if has_canonical_cite(reason) {
                            // cite-first (§8.3): reason contains a canonical #NNNN → β resolves it.
                            out.push((line_no, LineClass::Cited(extract_cites(reason)), line.trim().to_string()));
                        } else if has_blocker_prose(reason) {
                            out.push((line_no, LineClass::Structural(Kind::Untracked), line.trim().to_string()));
                        }
                        // else: operational reason → no entry (pass)
                    }
                    // extract_ignore_reason returned None (non-canonical form) → no entry
                }
            }
        } else if find_comment_marker(line).is_some() {
            // (3) comment markers (all swept exts).
            if has_canon {
                // canonical cite → tracked; β resolves the on-line cites. No
                // above-line lookback here (that is a stub-macro convention),
                // so an unrelated cite on the prior line cannot mask this one.
                out.push((line_no, LineClass::Cited(extract_cites(line)), line.trim().to_string()));
            } else if has_malformed_cite(line) {
                out.push((line_no, LineClass::Structural(Kind::MalformedCite), line.trim().to_string()));
            } else {
                out.push((line_no, LineClass::Structural(Kind::Untracked), line.trim().to_string()));
            }
        } else if is_rust && find_macro_stub(line) {
            // (4) stub macros (.rs only) with above-line cite lookback.
            let cited_above = prev.is_some_and(has_canonical_cite);
            if has_canon || cited_above {
                // tracked via on-line or above-line cite → β resolves the union.
                let mut ids = extract_cites(line);
                if let Some(p) = prev {
                    ids.extend(extract_cites(p));
                }
                dedup_in_place(&mut ids);
                out.push((line_no, LineClass::Cited(ids), line.trim().to_string()));
            } else {
                out.push((line_no, LineClass::Structural(Kind::Untracked), line.trim().to_string()));
            }
        } else if phantom_phrase(line) && !has_canon {
            // (5) phantom tracking — claim of tracking with no canonical cite.
            out.push((line_no, LineClass::Structural(Kind::PhantomTracking), line.trim().to_string()));
        }

        prev = Some(line);
    }
    out
}

/// Order-preserving in-place dedup of cite ids. Cite lists are tiny (1–2
/// elements), so the O(n²) membership scan is cheaper than a `HashSet`.
fn dedup_in_place(ids: &mut Vec<u32>) {
    let mut seen: Vec<u32> = Vec::new();
    ids.retain(|id| {
        if seen.contains(id) {
            false
        } else {
            seen.push(*id);
            true
        }
    });
}

// -----------------------------------------------------------------------
// §6.7 liveness lane — task-DB path resolution
// -----------------------------------------------------------------------

/// §6.7 task-DB path resolution: the `REIFY_PTODO_TASKS_DB` env override (used
/// verbatim when set and non-empty), else `<project_root>/.taskmaster/tasks/
/// tasks.db`. `std::env::var_os` is a *read*, which is safe under edition 2024
/// (unlike `set_var`); tests exercise the override only via subprocess env.
fn tasks_db_path(project_root: &Path) -> PathBuf {
    if let Some(v) = std::env::var_os("REIFY_PTODO_TASKS_DB")
        && !v.is_empty()
    {
        return PathBuf::from(v);
    }
    project_root.join(".taskmaster/tasks/tasks.db")
}

/// §6.7 read-only open of the task DB. `SQLITE_OPEN_READ_ONLY` never creates
/// the file and errors when it is absent (the degradation trigger), and dodges
/// the URI `file:…?mode=ro` path-escaping fragility on tempdir paths. An
/// existing-but-unreadable DB surfaces later as a prepare error in
/// [`resolve_liveness`], which also degrades.
fn open_tasks_db(path: &Path) -> rusqlite::Result<rusqlite::Connection> {
    rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
}

/// §8.4 terminal statuses: a cite resolving to one of these is "dead" and
/// orphans its marker. Every other present status (pending / in-progress /
/// blocked / deferred) is live (η later flips orphaned to High; β keeps all
/// liveness kinds Medium).
fn is_terminal_status(status: &str) -> bool {
    status == "done" || status == "cancelled"
}

// -----------------------------------------------------------------------
// §6.3 inverse lane — non-terminal tasks citing git-deleted metadata.files paths
// -----------------------------------------------------------------------

/// §6.3 inverse-lane membership test: returns `true` when `path` (trailing-
/// slash-tolerant) is "present in the tracked set" — i.e. it equals a tracked
/// file OR is a directory prefix of some tracked file (a tracked file starts
/// with `path + "/"`). Strips at most one trailing `/` before the checks.
///
/// This guard suppresses the critical FP class where `metadata.files` names
/// a DIRECTORY that still exists (e.g. `crates/reify-audit/tests`): a
/// directory is never a member of the `git ls-files` set, yet
/// `git log -1 -- <dir>` returns non-empty — without this guard, every
/// directory citation would produce a false-positive finding.
fn path_present_in_tracked(path: &str, tracked: &std::collections::HashSet<String>) -> bool {
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

/// §6.3 inverse lane: for each non-terminal master task, check each cited
/// `metadata.files` path. A path absent from `tracked` (not an exact tracked
/// file and not a directory prefix of one) is checked for git history via
/// [`crate::GitOps::last_commit_for_path`]:
///
/// - `Some(commit)` → the path was deleted → emit a Medium [`Pattern::PTodo`]
///   `task-cites-deleted-path` finding carrying the task id, the path, and
///   the last-touching commit.
/// - `None` → path never existed → presumed to-be-created → pass (no finding).
///
/// Fail-soft on DB errors (propagated as `Err` so the caller's
/// `and_then`-based degradation handles them alongside the liveness lane).
/// NULL/malformed/missing `metadata` → empty files list → graceful (no panic).
///
/// Findings are sorted by (task_id, path) for determinism; deleted paths are
/// by definition absent from `tracked` so they never share a key with the
/// structural/liveness (path, line) findings.
// G-allow: test-facing thin pub fn (mirrors resolve_liveness's pattern). MUST stay `pub`: its sole callers are the tests/ptodo.rs integration test binary (a SEPARATE crate — cannot see crate-private items) and check() (same module); `pub(crate)` would break the integration test, `#[cfg(test)]` would hide it from the same external caller.
pub fn resolve_inverse(
    conn: &rusqlite::Connection,
    git: &dyn crate::GitOps,
    tracked: &std::collections::HashSet<String>,
) -> rusqlite::Result<Vec<Finding>> {
    let mut stmt =
        conn.prepare("SELECT id, status, metadata FROM tasks WHERE tag = 'master'")?;

    let rows: Vec<(i64, String, Option<String>)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;

    let mut out: Vec<Finding> = Vec::new();
    // Per-run cache: avoid redundant `git log` spawns when multiple tasks cite
    // the same absent path (common in larger backlogs where a single deleted
    // file is referenced by several related tasks).
    let mut git_cache: HashMap<String, Option<GitCommit>> = HashMap::new();

    for (id, status, metadata_opt) in rows {
        if is_terminal_status(&status) {
            continue;
        }

        // Parse metadata.files: NULL / malformed / missing key → empty, graceful.
        let files: Vec<String> = metadata_opt
            .and_then(|m| serde_json::from_str::<serde_json::Value>(&m).ok())
            .and_then(|v| v.get("files").and_then(|a| a.as_array()).cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();

        for path in files {
            if path_present_in_tracked(&path, tracked) {
                continue;
            }
            // Path absent from tracked set — check git history (fail-safe: None
            // on any git error → no false positive). Results are memoized to
            // avoid repeated subprocess spawns for the same path across tasks.
            let commit_opt = git_cache
                .entry(path.clone())
                .or_insert_with(|| git.last_commit_for_path(&path))
                .clone();
            if let Some(commit) = commit_opt {
                out.push(Finding {
                    pattern: Pattern::PTodo,
                    severity: Severity::Medium,
                    task_id: id.to_string(),
                    summary: format!(
                        "task-cites-deleted-path: task #{id} cites deleted path '{path}' (last touched {sha})",
                        sha = commit.sha,
                    ),
                    evidence: vec![
                        EvidenceRef::MetadataFiles { entries: vec![path.clone()] },
                        EvidenceRef::Commit { sha: commit.sha, subject: commit.subject },
                    ],
                });
            }
            // None → path never existed → presumed to-be-created → pass.
        }
    }

    // Deterministic order: (task_id parsed as integer, path). Deleted paths are
    // absent from `tracked` so there is no cross-lane sort key collision.
    out.sort_by(|a, b| {
        let id_a = a.task_id.parse::<i64>().unwrap_or(i64::MAX);
        let id_b = b.task_id.parse::<i64>().unwrap_or(i64::MAX);
        let path_a = a
            .evidence
            .iter()
            .find_map(|e| {
                if let EvidenceRef::MetadataFiles { entries } = e {
                    entries.first().cloned()
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let path_b = b
            .evidence
            .iter()
            .find_map(|e| {
                if let EvidenceRef::MetadataFiles { entries } = e {
                    entries.first().cloned()
                } else {
                    None
                }
            })
            .unwrap_or_default();
        id_a.cmp(&id_b).then(path_a.cmp(&path_b))
    });

    Ok(out)
}

/// §8.2/§8.3 liveness resolution: per cited marker, resolve each `#NNNN` id's
/// status against the task DB and classify.
///
/// §8.2 multi-cite rule — "one live cite suffices for tracking": if ANY cite
/// resolves to a present non-terminal status the marker is tracked and emits
/// nothing. Otherwise every dead cite is explained — a present terminal cite
/// (done / cancelled) → one `orphaned` finding (summary carries `#id` +
/// status); an absent cite → one `unknown-id` finding. All findings are
/// [`Pattern::PTodo`] / [`Severity::Medium`] (§8.4) with `task_id = path` and a
/// single [`EvidenceRef::File`] ref.
///
/// A statement-prepare error (missing `tasks` table / corrupt DB) is propagated
/// as `Err` so [`check`] degrades fail-soft (§6.7) instead of panicking.
// G-allow: test-facing thin wrapper over `resolve_liveness_keyed`. MUST stay `pub` (not `pub(crate)`/`#[cfg(test)]`): its sole caller is the tests/ptodo.rs integration test — a SEPARATE crate that cannot see crate-private or cfg(test)-gated items — while production `check` calls the keyed variant directly.
pub fn resolve_liveness(
    conn: &rusqlite::Connection,
    cited: &[(String, usize, Vec<u32>, String)],
) -> rusqlite::Result<Vec<Finding>> {
    Ok(resolve_liveness_keyed(conn, cited)?
        .into_iter()
        .map(|(_path, _line, finding)| finding)
        .collect())
}

/// Internal variant of [`resolve_liveness`] that tags each finding with its
/// `(path, line)` sort key, so [`check`] can merge the liveness findings with
/// the structural ones into a single deterministic `(path, line)`-ordered
/// stream. [`resolve_liveness`] is the thin public wrapper that drops the keys;
/// the findings and their order are identical either way.
fn resolve_liveness_keyed(
    conn: &rusqlite::Connection,
    cited: &[(String, usize, Vec<u32>, String)],
) -> rusqlite::Result<Vec<(String, usize, Finding)>> {
    // §6.7 normative (PRD `reify-audit-ptodo-detector.md` line 181, "rows
    // filtered to `tag='master'`"): the reify task DB uses the single canonical
    // `master` tag context, so a cite is resolved ONLY there. Consequence — an id
    // that exists solely under a non-master tag is invisible to this query and
    // classifies as `unknown-id` (neither tracked nor orphaned); this is the
    // intended master-only semantics, pinned by the integration test
    // `liveness::non_master_tag_resolves_as_unknown_id` (tests/ptodo.rs). Should a
    // multi-tag task DB ever be introduced, revisit this filter alongside §8.2.
    let mut stmt = conn.prepare("SELECT status FROM tasks WHERE tag = 'master' AND id = ?1")?;
    let mut out = Vec::new();

    for (path, line, ids, text) in cited {
        // Resolve every cite once; remember each id's status (None = absent).
        let mut resolved: Vec<(u32, Option<String>)> = Vec::with_capacity(ids.len());
        let mut any_live = false;
        for &id in ids {
            let status: Option<String> = stmt
                .query_row(rusqlite::params![id], |row| row.get::<_, String>(0))
                .optional()?;
            if status.as_deref().is_some_and(|s| !is_terminal_status(s)) {
                any_live = true;
            }
            resolved.push((id, status));
        }

        // §8.2: a single live cite tracks the whole marker → no finding.
        if any_live {
            continue;
        }

        for (id, status) in resolved {
            let finding = match status {
                // Present and — since !any_live — necessarily terminal → orphaned.
                Some(s) => liveness_finding(
                    path,
                    format!("orphaned: line {line}: #{id} status={s}: {text}"),
                ),
                // Absent → unknown-id.
                None => liveness_finding(
                    path,
                    format!("unknown-id: line {line}: #{id}: {text}"),
                ),
            };
            out.push((path.clone(), *line, finding));
        }
    }

    Ok(out)
}

/// Build a Medium PTODO liveness [`Finding`] at `path` with the given summary.
fn liveness_finding(path: &str, summary: String) -> Finding {
    Finding {
        pattern: Pattern::PTodo,
        severity: Severity::Medium,
        summary,
        task_id: path.to_string(),
        evidence: vec![EvidenceRef::File { path: path.to_string() }],
    }
}

// -----------------------------------------------------------------------
// §5 detector entry point — working-tree sweep
// -----------------------------------------------------------------------

/// PTODO sweep (§5/§8) — both lanes. Enumerates tracked files via the git seam
/// ([`GitOps::ls_files`](crate::GitOps::ls_files)), keeps only swept extensions
/// that are not allowlisted (§6.8), reads each file's **working-tree** content
/// directly (`std::fs::read_to_string` — only enumeration is a git dependency;
/// the lane "runs everywhere, including worktrees"), and classifies each line via
/// the single [`scan_file`] pass.
///
/// That one pass feeds both lanes: [`LineClass::Structural`] lines become α
/// structural findings; [`LineClass::Cited`] markers are resolved against the
/// task DB by the β liveness lane. The task DB is opened read-only at
/// [`tasks_db_path`]; when it is absent or unreadable the liveness lane is
/// skipped and only the structural findings are returned (the §6.7 fail-soft
/// breadcrumb is wired in a later step).
///
/// Every finding is Medium severity with `task_id` = file path and a summary of
/// the §8.3 `"<kind>: line N: <text>"` prefix form. Unreadable paths (deleted /
/// binary / permission) are skipped fail-safe. Findings are returned in
/// deterministic `(path, line)` order across both lanes.
pub fn check(ctx: &AuditContext) -> Vec<Finding> {
    // Structural offenders (α) and cited markers (β) from the single scan_file
    // pass, kept separate so each feeds its own lane.
    let mut struct_hits: Vec<(String, usize, Kind, String)> = Vec::new();
    let mut cited: Vec<(String, usize, Vec<u32>, String)> = Vec::new();

    // Collect ls_files() once: the Vec drives the structural sweep; the HashSet
    // is reused by the ζ inverse-lane membership test without a second git call.
    let tracked_files: Vec<String> = ctx.git.ls_files();
    let tracked_set: HashSet<String> = tracked_files.iter().cloned().collect();

    for path in &tracked_files {
        if !is_swept_ext(path) || is_allowlisted(path) {
            continue;
        }
        // Read the working tree directly (only enumeration is a git seam). Skip
        // unreadable paths fail-safe.
        let content = match std::fs::read_to_string(ctx.project_root.join(path)) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let is_rust = path.ends_with(".rs");
        for (line_no, class, text) in scan_file(&content, is_rust) {
            match class {
                LineClass::Structural(kind) => struct_hits.push((path.clone(), line_no, kind, text)),
                LineClass::Cited(ids) => cited.push((path.clone(), line_no, ids, text)),
            }
        }
    }

    // α structural findings, each tagged with its (path, line) sort key.
    let mut keyed: Vec<(String, usize, Finding)> = struct_hits
        .into_iter()
        .map(|(path, line_no, kind, text)| {
            let finding = Finding {
                pattern: Pattern::PTodo,
                severity: Severity::Medium,
                summary: format!("{}: line {}: {}", kind.as_str(), line_no, text),
                task_id: path.clone(),
                evidence: vec![EvidenceRef::File { path: path.clone() }],
            };
            (path, line_no, finding)
        })
        .collect();

    // β liveness lane + ζ inverse lane: open the task DB read-only; on success
    // resolve BOTH the collected cites (β) AND the inverse-path check (ζ) so
    // they degrade together under the single existing breadcrumb (§6.7).
    // A missing/unreadable DB (open error) OR a prepare/probe failure on an
    // existing-but-corrupt DB (resolve error) degrades both lanes fail-soft.
    // The exit class is untouched (Medium-neutral) — 125 is reserved for genuine
    // arg/IO misconfig, never an absent optional substrate.
    let db_path = tasks_db_path(&ctx.project_root);
    let mut inverse_findings: Vec<Finding> = Vec::new();
    match open_tasks_db(&db_path).and_then(|conn| {
        let live = resolve_liveness_keyed(&conn, &cited)?;
        let inv = resolve_inverse(&conn, ctx.git, &tracked_set)?;
        Ok((live, inv))
    }) {
        Ok((live, inv)) => {
            keyed.extend(live);
            inverse_findings = inv;
        }
        Err(_) => eprintln!(
            "reify-audit: tasks.db unreachable at '{}' — PTODO liveness (β) and inverse (ζ) lanes degraded; structural checks still run",
            db_path.display()
        ),
    }

    // Deterministic merged order across structural + liveness lanes: (path, line).
    // A given line yields at most one lane's entry (scan_file emits one LineClass
    // per line), so there is no cross-lane tie; the stable sort preserves the
    // per-marker multi-cite order within a line.
    keyed.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut out: Vec<Finding> = keyed.into_iter().map(|(_path, _line, finding)| finding).collect();
    // ζ inverse findings are already sorted by (task_id, path); append as a
    // deterministic trailing block. Deleted paths are absent from tracked_set
    // so they never share a (path,line) sort key with structural/liveness findings.
    out.extend(inverse_findings);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test-only derivation of the structural lane: [`scan_file`] filtered to its
    /// [`LineClass::Structural`] entries (the `Cited` markers — β's domain — drop
    /// out), yielding one `(line_no, kind, text)` per structurally-offending line.
    /// Production [`check`] drives [`scan_file`] directly (it needs the `Cited`
    /// markers this filter discards), so this "structural = scan_file ∩ Structural"
    /// view lives here purely to exercise α's precedence unit tests.
    fn classify_file(content: &str, is_rust: bool) -> Vec<(usize, Kind, String)> {
        scan_file(content, is_rust)
            .into_iter()
            .filter_map(|(line_no, class, text)| match class {
                LineClass::Structural(kind) => Some((line_no, kind, text)),
                LineClass::Cited(_) => None,
            })
            .collect()
    }

    // -------------------------------------------------------------------
    // §8.1 marker recognition — comment markers
    // -------------------------------------------------------------------

    #[test]
    fn comment_marker_positives() {
        assert_eq!(find_comment_marker("// TODO: x"), Some("TODO"));
        assert_eq!(find_comment_marker("// FIXME(y)"), Some("FIXME"));
        assert_eq!(find_comment_marker("HACK : z"), Some("HACK"));
        assert_eq!(find_comment_marker("# TODO: sh-comment"), Some("TODO"));
    }

    #[test]
    fn comment_marker_negatives() {
        // Followed by space+word, not `(`/`:`.
        assert_eq!(find_comment_marker("// the TODO extractor"), None);
        // Lowercase — case-sensitive uppercase only.
        assert_eq!(find_comment_marker("// todo: someday"), None);
        // No left word boundary (preceded by `X`).
        assert_eq!(find_comment_marker("// XTODO: x"), None);
        // Right boundary violated — `TODONE` is not the whole word `TODO`.
        assert_eq!(find_comment_marker("// TODONE: x"), None);
    }

    // -------------------------------------------------------------------
    // §8.1 marker recognition — macro stubs (.rs)
    // -------------------------------------------------------------------

    #[test]
    fn macro_stub_positives_and_negative() {
        assert!(find_macro_stub("    todo!()"));
        assert!(find_macro_stub("    unimplemented!(\"later\")"));
        assert!(!find_macro_stub("    let x = compute();"));
        // Commented-out / doc-comment mentions are prose, not real stubs.
        assert!(!find_macro_stub("// todo!() example"));
        assert!(!find_macro_stub("/// returns todo!() placeholder"));
    }

    // -------------------------------------------------------------------
    // §8.3 γ blocker-prose matching — has_blocker_prose
    // -------------------------------------------------------------------

    #[test]
    fn has_blocker_prose_positives() {
        // "pending" — case-insensitive
        assert!(has_blocker_prose("pending fillet binding"));
        assert!(has_blocker_prose("Pending upstream fix"));
        // "not yet" — case-insensitive
        assert!(has_blocker_prose("not yet implemented"));
        assert!(has_blocker_prose("Not Yet ready"));
        // "RED:" — case-SENSITIVE (must stay uppercase)
        assert!(has_blocker_prose("RED: awaiting impl"));
        // "until " — case-insensitive (trailing space is part of needle)
        assert!(has_blocker_prose("ignore until fillet lands"));
        assert!(has_blocker_prose("Until some later date"));
        // "once " — case-insensitive (trailing space is part of needle)
        assert!(has_blocker_prose("run once manually"));
        assert!(has_blocker_prose("Once fixed, remove this"));
        // "blocked" — case-insensitive
        assert!(has_blocker_prose("blocked on upstream"));
        assert!(has_blocker_prose("Blocked by refactor"));
    }

    #[test]
    fn has_blocker_prose_negatives() {
        // Operational reasons — none of the needles present
        assert!(!has_blocker_prose("requires OCCT"));
        assert!(!has_blocker_prose("probe: run manually"));
        assert!(!has_blocker_prose("timing/benchmark out of CI"));
        // Case-sensitivity guard: "required:" contains "red:" in lowercase
        // but must NOT match because RED: is matched case-sensitively.
        assert!(!has_blocker_prose("required: rebuild"));
        // Empty reason
        assert!(!has_blocker_prose(""));
    }

    // -------------------------------------------------------------------
    // §8.1 marker recognition — ignore attributes (.rs)
    // -------------------------------------------------------------------

    #[test]
    fn ignore_attr_forms() {
        assert_eq!(ignore_attr("#[ignore]"), Some(IgnoreForm::Bare));
        assert_eq!(ignore_attr("#[ignore = \"r\"]"), Some(IgnoreForm::WithReason));
        // Indented bare form still recognised (trimmed-line start).
        assert_eq!(ignore_attr("    #[ignore]"), Some(IgnoreForm::Bare));
        // Doc-comment prose mentioning the attribute must NOT fire.
        assert_eq!(ignore_attr("/// #[ignore]"), None);
    }

    // -------------------------------------------------------------------
    // §8.2 citation resolution — canonical `#NNNN`
    // -------------------------------------------------------------------

    #[test]
    fn canonical_cite_positives() {
        assert!(has_canonical_cite("// TODO(#42): x"));
        assert!(has_canonical_cite("see #4553"));
        assert!(has_canonical_cite("#1"));
        assert!(has_canonical_cite("#12345 five digits"));
    }

    #[test]
    fn canonical_cite_negatives() {
        assert!(!has_canonical_cite("bare # alone"));
        assert!(!has_canonical_cite("#abc not digits"));
        // 6-digit run exceeds the 1..=5 window — not a 5-digit prefix match.
        assert!(!has_canonical_cite("#123456 six digits"));
        // Space between `#` and digits.
        assert!(!has_canonical_cite("# 42"));
        // All-zero runs (`#0`, `#00`) are not valid task ids (ids start at 1).
        assert!(!has_canonical_cite("// TODO(#0): x"));
        assert!(!has_canonical_cite("see #00 here"));
    }

    // -------------------------------------------------------------------
    // §8.2 cite extraction (β liveness lane) — `extract_cites`
    // -------------------------------------------------------------------

    #[test]
    fn extract_cites_collects_all_canonical_ids() {
        // A single parenthesised cite.
        assert_eq!(extract_cites("// TODO(#42): x"), vec![42]);
        // Multiple bare cites in source order.
        assert_eq!(extract_cites("see #1 and #200"), vec![1, 200]);
        // Leading zeros are tolerated as long as the value is ≥1 (`#007` → 7).
        assert_eq!(extract_cites("// TODO(#007): x"), vec![7]);
    }

    #[test]
    fn extract_cites_rejects_non_cites() {
        // `#` followed by non-digits → no cite.
        assert_eq!(extract_cites("#abc"), Vec::<u32>::new());
        // A bare `#` at line end → no cite.
        assert_eq!(extract_cites("bare #"), Vec::<u32>::new());
        // A 6-digit run exceeds the 1..=5 window (consistent with
        // has_canonical_cite) → no cite (not a 5-digit prefix match).
        assert_eq!(extract_cites("#123456"), Vec::<u32>::new());
        // An all-zero run is not a valid task id (ids start at 1) → no cite, so
        // `#0` falls through to the structural `untracked` classification.
        assert_eq!(extract_cites("#0"), Vec::<u32>::new());
        assert_eq!(extract_cites("// TODO(#00): x"), Vec::<u32>::new());
    }

    // -------------------------------------------------------------------
    // §8.2/§6.4 malformed citations — Greek / PRD-relative / legacy
    // -------------------------------------------------------------------

    #[test]
    fn malformed_cite_positives() {
        assert!(has_malformed_cite("// TODO(task δ): migrate")); // Greek
        assert!(has_malformed_cite("tracked in task α")); // Greek, no space-after-paren
        assert!(has_malformed_cite("// TODO(task-5): later")); // PRD-relative
        assert!(has_malformed_cite("// TODO: see task 4553")); // legacy space form
        assert!(has_malformed_cite("// TODO: see task_4553")); // legacy underscore form
    }

    #[test]
    fn malformed_cite_negatives() {
        // Canonical-only line must not be reported malformed (no `task` token).
        assert!(!has_malformed_cite("// TODO(#4553): migrate"));
        // Ordinary prose, no task+cite shape.
        assert!(!has_malformed_cite("the multitasking scheduler runs"));
        // A bare canonical cite.
        assert!(!has_malformed_cite("resolved in #4553"));
        // `task` embedded in a larger word (no left boundary) must NOT match,
        // even when followed by a separator + digit (`multitask 5`).
        assert!(!has_malformed_cite("// TODO: schedule multitask 5 jobs"));
    }

    // -------------------------------------------------------------------
    // §8.3 phantom-tracking phrases (case-insensitive)
    // -------------------------------------------------------------------

    #[test]
    fn phantom_phrase_positives() {
        // All four normative phrases.
        assert!(phantom_phrase("this is tracked separately"));
        assert!(phantom_phrase("// tracked as a follow-up task"));
        assert!(phantom_phrase("tracked in project memory for later"));
        assert!(phantom_phrase("a follow-up task will handle this"));
        // Mixed-case variant — matching is case-insensitive.
        assert!(phantom_phrase("// Tracked As A Follow-Up task"));
    }

    #[test]
    fn phantom_phrase_negative() {
        // Ordinary prose that mentions tracking but not a phantom phrase.
        assert!(!phantom_phrase("// the tracker walks the working tree"));
    }

    // -------------------------------------------------------------------
    // §6.8 inline escape — `ptodo:allow`
    // -------------------------------------------------------------------

    #[test]
    fn line_escaped_detects_marker() {
        assert!(line_escaped("// TODO: leave me  // ptodo:allow"));
        assert!(!line_escaped("// TODO: flag me"));
    }

    // -------------------------------------------------------------------
    // §6.8 allowlist prefixes
    // -------------------------------------------------------------------

    #[test]
    fn allowlist_membership() {
        // The detector's own crate (pattern strings + committed fixtures).
        assert!(is_allowlisted("crates/reify-audit/src/p2_consumer_stub.rs"));
        // The #[ignore]-extraction tool and its tests.
        assert!(is_allowlisted("crates/reify-test-support/src/ignore_hygiene.rs"));
        assert!(is_allowlisted(
            "crates/reify-test-support/tests/ignore_reason_hygiene.rs"
        ));
        // An ordinary crate source path is NOT allowlisted.
        assert!(!is_allowlisted("crates/reify-ast/src/decl.rs"));
    }

    // -------------------------------------------------------------------
    // §6.8 swept extensions
    // -------------------------------------------------------------------

    #[test]
    fn swept_extension_membership() {
        for p in ["a.rs", "b.ri", "c.sh", "d.py", "e.ts", "f.tsx", "g.js"] {
            assert!(is_swept_ext(p), "{p} should be a swept extension");
        }
        for p in ["a.md", "b.toml", "c.yaml", "d.json"] {
            assert!(!is_swept_ext(p), "{p} should NOT be a swept extension");
        }
    }

    // -------------------------------------------------------------------
    // §8 per-file classification orchestration (precedence)
    // -------------------------------------------------------------------

    #[test]
    fn classify_file_precedence_rust() {
        // Each line exercises exactly one §8 precedence rule; line numbers are
        // 1-based. is_rust=true so macro/ignore rules are live.
        let lines = [
            "// TODO(#4553): cited",              // 1 (a) canonical cite -> no entry
            "// tracked as a follow-up task",     // 2 (b) phantom, no cite -> PhantomTracking
            "// TODO(task δ): migrate",           // 3 (c) marker + malformed -> MalformedCite
            "// TODO: wire this",                 // 4 (d) marker, no cite -> Untracked
            "    #[ignore]",                      // 5 (e) bare ignore -> BareIgnore
            "    #[ignore = \"blocked\"]",        // 6 (f) blocker-prose reason -> Untracked (γ)
            "// resolved in #4553",               // 7 canonical cite, no marker -> no entry (prev for 8)
            "    todo!()",                        // 8 (g) macro, canonical cite directly above -> no entry
            "// TODO: leave me  // ptodo:allow",  // 9 (h) inline escape -> skipped
            "    todo!(\"later\")",               // 10 macro, no cite above -> Untracked
        ];
        let content = lines.join("\n");
        let got = classify_file(&content, true);

        let expected: Vec<(usize, Kind, String)> = vec![
            (2, Kind::PhantomTracking, "// tracked as a follow-up task".to_string()),
            (3, Kind::MalformedCite, "// TODO(task δ): migrate".to_string()),
            (4, Kind::Untracked, "// TODO: wire this".to_string()),
            (5, Kind::BareIgnore, "#[ignore]".to_string()),
            // γ: blocker-prose reason "blocked" → Untracked (was "no entry" pre-γ).
            (6, Kind::Untracked, "#[ignore = \"blocked\"]".to_string()),
            (10, Kind::Untracked, "todo!(\"later\")".to_string()),
        ];
        assert_eq!(got, expected);
    }

    // -------------------------------------------------------------------
    // §8.3 γ structural policy — blocker-prose vs operational
    // -------------------------------------------------------------------

    /// Blocker-prose reason (no cite) → Structural(Untracked).
    /// Operational reason (no cite, no blocker-prose) → no entry.
    /// Bare #[ignore] → Structural(BareIgnore) (regression).
    #[test]
    fn scan_file_ignore_with_reason_blocker_prose_and_operational() {
        let lines = [
            "#[ignore = \"pending fillet binding\"]", // 1 blocker-prose -> Structural(Untracked)
            "#[ignore = \"requires OCCT\"]",           // 2 operational -> no entry
            "#[ignore]",                              // 3 bare -> Structural(BareIgnore)
        ];
        let content = lines.join("\n");
        let got = scan_file(&content, true);

        let expected: Vec<(usize, LineClass, String)> = vec![
            (1, LineClass::Structural(Kind::Untracked),
             "#[ignore = \"pending fillet binding\"]".to_string()),
            (3, LineClass::Structural(Kind::BareIgnore), "#[ignore]".to_string()),
        ];
        assert_eq!(got, expected);
    }

    /// Non-canonical `#[ignore="blocked"]` (no spaces around `=`) — identical
    /// to the canonical form from scan_file's perspective: extract_ignore_reason
    /// mirrors ignore_attr's tolerance of non-spaced forms.
    #[test]
    fn scan_file_ignore_non_canonical_form_blocker_prose() {
        let content = "#[ignore=\"pending fillet binding\"]";
        let got = scan_file(content, true);
        assert_eq!(
            got,
            vec![(
                1,
                LineClass::Structural(Kind::Untracked),
                "#[ignore=\"pending fillet binding\"]".to_string(),
            )]
        );
    }

    // -------------------------------------------------------------------
    // §8 unified scan — `scan_file` (Structural + Cited) (β liveness lane)
    // -------------------------------------------------------------------

    #[test]
    fn scan_file_emits_cited_and_structural() {
        // is_rust=true so the macro / #[ignore] rules are live.
        let lines = [
            "// TODO(#4553): x",          // 1 comment marker + canonical cite -> Cited([4553])
            "// #42",                     // 2 cite-only, no marker -> no entry (prev for 3)
            "    todo!()",                // 3 stub macro, cite directly above -> Cited([42])
            "    #[ignore = \"see #42\"]", // 4 reason-bearing with cite -> Cited([42]) (γ cite-first)
            "// TODO: wire this",         // 5 marker, no cite -> Structural(Untracked)
            "// TODO(#5): x  // ptodo:allow", // 6 inline escape on a cited line -> skipped
        ];
        let content = lines.join("\n");

        let got = scan_file(&content, true);
        let expected: Vec<(usize, LineClass, String)> = vec![
            (1, LineClass::Cited(vec![4553]), "// TODO(#4553): x".to_string()),
            (3, LineClass::Cited(vec![42]), "todo!()".to_string()),
            // γ cite-first: reason "see #42" has a canonical cite → Cited([42]).
            (4, LineClass::Cited(vec![42]), "#[ignore = \"see #42\"]".to_string()),
            (5, LineClass::Structural(Kind::Untracked), "// TODO: wire this".to_string()),
        ];
        assert_eq!(got, expected);

        // Regression: classify_file is exactly scan_file filtered to its
        // Structural variants — the Cited markers (1, 3, 4) and the suppressed
        // lines (2, 6) drop out, leaving byte-identical α output.
        let classified = classify_file(&content, true);
        let expected_structural: Vec<(usize, Kind, String)> =
            vec![(5, Kind::Untracked, "// TODO: wire this".to_string())];
        assert_eq!(classified, expected_structural);
    }

    // -------------------------------------------------------------------
    // §8.3 γ cite-first path — reason with canonical cite → Cited (β lane)
    // -------------------------------------------------------------------

    /// `#[ignore = "blocked on #4444"]` — cite wins over blocker-prose.
    /// `#[ignore = "see #42"]`          — cite without blocker-prose.
    #[test]
    fn scan_file_ignore_reason_with_cite_emits_cited_entry() {
        let lines = [
            "#[ignore = \"blocked on #4444\"]", // 1 cite wins over "blocked" prose → Cited([4444])
            "#[ignore = \"see #42\"]",          // 2 cite, no blocker-prose → Cited([42])
        ];
        let content = lines.join("\n");
        let got = scan_file(&content, true);

        let expected: Vec<(usize, LineClass, String)> = vec![
            (1, LineClass::Cited(vec![4444]), "#[ignore = \"blocked on #4444\"]".to_string()),
            (2, LineClass::Cited(vec![42]),   "#[ignore = \"see #42\"]".to_string()),
        ];
        assert_eq!(got, expected);
    }

    // -------------------------------------------------------------------
    // §6.7 task-DB path resolution (β liveness lane) — `tasks_db_path`
    // -------------------------------------------------------------------

    #[test]
    fn tasks_db_path_defaults_under_project_root() {
        // With REIFY_PTODO_TASKS_DB unset (the normal cargo-test env), the path
        // resolves to <project_root>/.taskmaster/tasks/tasks.db. The env-override
        // branch is covered end-to-end by the subprocess test (no unsafe set_var).
        assert_eq!(
            tasks_db_path(std::path::Path::new("/repo")),
            std::path::PathBuf::from("/repo/.taskmaster/tasks/tasks.db"),
        );
    }

    #[test]
    fn classify_file_non_rust_skips_macro_and_ignore() {
        // is_rust=false: comment markers and phantom phrases still fire (all
        // swept exts), but the .rs-only macro and #[ignore] rules do NOT.
        let lines = [
            "# TODO: wire this sh script", // 1 comment marker -> Untracked
            "todo!()",                     // 2 macro -> suppressed (is_rust=false)
            "#[ignore]",                   // 3 ignore -> suppressed (is_rust=false)
            "tracked separately",          // 4 phantom -> PhantomTracking
        ];
        let content = lines.join("\n");
        let got = classify_file(&content, false);

        let expected: Vec<(usize, Kind, String)> = vec![
            (1, Kind::Untracked, "# TODO: wire this sh script".to_string()),
            (4, Kind::PhantomTracking, "tracked separately".to_string()),
        ];
        assert_eq!(got, expected);
    }

    // -------------------------------------------------------------------
    // §6.6 fingerprint() — baseline fingerprint derivation
    // -------------------------------------------------------------------

    /// (a) Structural finding: line-N stripped, internal whitespace folded.
    #[test]
    fn fingerprint_structural_untracked() {
        let finding = Finding {
            pattern: Pattern::PTodo,
            severity: Severity::Medium,
            task_id: "crates/foo/bar.rs".to_string(),
            summary: "untracked: line 12:    // TODO: wire   this".to_string(),
            evidence: vec![],
        };
        assert_eq!(
            fingerprint(&finding),
            "crates/foo/bar.rs :: untracked :: // TODO: wire this",
        );
    }

    /// (b) Malformed-cite finding: same stripping/folding rules as structural.
    #[test]
    fn fingerprint_structural_malformed_cite() {
        let finding = Finding {
            pattern: Pattern::PTodo,
            severity: Severity::Medium,
            task_id: "crates/reify-eval/src/dispatcher.rs".to_string(),
            summary: "malformed-cite: line 5: // TODO(task-3445): some  text".to_string(),
            evidence: vec![],
        };
        assert_eq!(
            fingerprint(&finding),
            "crates/reify-eval/src/dispatcher.rs :: malformed-cite :: // TODO(task-3445): some text",
        );
    }

    /// (c) Liveness finding: kind up to first ':', `line N: ` stripped, rest kept verbatim
    /// modulo whitespace folding. The `orphaned` kind has additional structure
    /// (`#id status=done: <text>`) that must be preserved.
    #[test]
    fn fingerprint_liveness_orphaned() {
        let finding = Finding {
            pattern: Pattern::PTodo,
            severity: Severity::Medium,
            task_id: "crates/reify-eval/src/engine_purposes.rs".to_string(),
            summary: "orphaned: line 7: #4551 status=done: // FIXME(#4551): x".to_string(),
            evidence: vec![],
        };
        assert_eq!(
            fingerprint(&finding),
            "crates/reify-eval/src/engine_purposes.rs :: orphaned :: #4551 status=done: // FIXME(#4551): x",
        );
    }

    /// Unknown-id liveness finding: `unknown-id` kind, `line N: #id: <text>`.
    #[test]
    fn fingerprint_liveness_unknown_id() {
        let finding = Finding {
            pattern: Pattern::PTodo,
            severity: Severity::Medium,
            task_id: "crates/reify-solver/src/lib.rs".to_string(),
            summary: "unknown-id: line 99: #9999: // TODO(#9999): placeholder".to_string(),
            evidence: vec![],
        };
        assert_eq!(
            fingerprint(&finding),
            "crates/reify-solver/src/lib.rs :: unknown-id :: #9999: // TODO(#9999): placeholder",
        );
    }
}
