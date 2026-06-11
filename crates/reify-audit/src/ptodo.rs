//! PTODO — TODO-tracking-invariant detector (structural lane, task α).
//!
//! Scans the working tree for TODO-family markers that are not backed by a
//! canonical `#NNNN` task citation, emitting Medium-severity findings. The
//! grammar lives in pure `&str -> result` functions (mirroring P2's
//! `line_matches_stub`/`scan_file_added_lines` split, no `regex` dependency
//! per design §12); only file enumeration (`GitOps::ls_files`) and content
//! reads (`std::fs::read_to_string`) touch IO, inside [`check`].
//!
//! Reference: `docs/prds/reify-audit-ptodo-detector.md` §8 (normative grammar).

use crate::{AuditContext, EvidenceRef, Finding, Pattern, Severity};

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
/// the `.rs`-only gating lives in [`classify_file`]. A line whose trimmed start
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

/// §8.1 ignore attributes (`.rs` only — gating in [`classify_file`]): a trimmed
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
/// 6-digit number is not matched on its 5-digit prefix).
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
            if (1..=5).contains(&run) {
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
/// consistent with the canonical-cite recogniser). `#0` parses to `0`.
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
                // in u32 (max 99999), so the parse cannot fail.
                if let Ok(id) = line[i + 1..j].parse::<u32>() {
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
/// is applied by the caller ([`classify_file`]).
fn phantom_phrase(line: &str) -> bool {
    let lower = line.to_lowercase();
    PHANTOM_PHRASES.iter().any(|p| lower.contains(p))
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

/// §8 per-file classification: scan `content` line-by-line and return one
/// `(line_no, kind, marker_text)` entry per offending line (1-based line
/// numbers, `marker_text` is the trimmed line). `is_rust` gates the `.rs`-only
/// macro and `#[ignore]` rules.
///
/// Precedence per line (first match wins; at most one entry per line):
/// 1. `ptodo:allow` inline escape → the line is skipped entirely (§6.8).
/// 2. `#[ignore]` (`.rs`): bare → `BareIgnore`; reason-bearing → no entry
///    (deferred to γ). Checked before comment markers so a reason string is
///    not misread as a marker.
/// 3. comment marker (all exts): canonical `#NNNN` → no entry (tracked, β
///    liveness-checks); malformed cite → `MalformedCite`; else `Untracked`.
/// 4. stub macro (`.rs`): a canonical cite on this line OR the line directly
///    above → no entry (above-line lookback); else `Untracked`.
/// 5. phantom phrase with no canonical cite → `PhantomTracking`.
fn classify_file(content: &str, is_rust: bool) -> Vec<(usize, Kind, String)> {
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
            // (2) #[ignore] (.rs only). Reason-bearing forms defer to γ.
            if form == IgnoreForm::Bare {
                out.push((line_no, Kind::BareIgnore, line.trim().to_string()));
            }
        } else if find_comment_marker(line).is_some() {
            // (3) comment markers (all swept exts).
            if has_canon {
                // canonical cite → tracked; liveness deferred to β.
            } else if has_malformed_cite(line) {
                out.push((line_no, Kind::MalformedCite, line.trim().to_string()));
            } else {
                out.push((line_no, Kind::Untracked, line.trim().to_string()));
            }
        } else if is_rust && find_macro_stub(line) {
            // (4) stub macros (.rs only) with above-line cite lookback.
            let cited_above = prev.is_some_and(has_canonical_cite);
            if !has_canon && !cited_above {
                out.push((line_no, Kind::Untracked, line.trim().to_string()));
            }
        } else if phantom_phrase(line) && !has_canon {
            // (5) phantom tracking — claim of tracking with no canonical cite.
            out.push((line_no, Kind::PhantomTracking, line.trim().to_string()));
        }

        prev = Some(line);
    }
    out
}

// -----------------------------------------------------------------------
// §5 detector entry point — working-tree sweep
// -----------------------------------------------------------------------

/// PTODO structural-lane sweep (§5/§8). Enumerates tracked files via the git
/// seam ([`GitOps::ls_files`](crate::GitOps::ls_files)), keeps only swept
/// extensions that are not allowlisted (§6.8), reads each file's **working-tree**
/// content directly (`std::fs::read_to_string` — only enumeration is a git
/// dependency; the lane "runs everywhere, including worktrees"), and classifies
/// each line via [`classify_file`].
///
/// Every offending line becomes one Medium-severity [`Finding`] whose `task_id`
/// is the file path and whose summary is the §8.3 `"<kind>: line N: <text>"`
/// prefix form. Unreadable paths (deleted / binary / permission) are skipped
/// fail-safe. Findings are returned in deterministic `(path, line_no)` order.
pub fn check(ctx: &AuditContext) -> Vec<Finding> {
    // (path, line_no, kind, marker_text) for every offending line.
    let mut hits: Vec<(String, usize, Kind, String)> = Vec::new();

    for path in ctx.git.ls_files() {
        if !is_swept_ext(&path) || is_allowlisted(&path) {
            continue;
        }
        // Structural lane reads the working tree directly (only enumeration is
        // a git seam). Skip unreadable paths fail-safe.
        let content = match std::fs::read_to_string(ctx.project_root.join(&path)) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let is_rust = path.ends_with(".rs");
        for (line_no, kind, text) in classify_file(&content, is_rust) {
            hits.push((path.clone(), line_no, kind, text));
        }
    }

    // Deterministic ordering: (path, line_no). Line numbers are excluded from
    // the task_id identity but kept in the human-readable summary detail.
    hits.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    hits.into_iter()
        .map(|(path, line_no, kind, text)| Finding {
            pattern: Pattern::PTodo,
            severity: Severity::Medium,
            summary: format!("{}: line {}: {}", kind.as_str(), line_no, text),
            task_id: path.clone(),
            evidence: vec![EvidenceRef::File { path }],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // `#0` is a valid 1-digit run → 0.
        assert_eq!(extract_cites("#0"), vec![0]);
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
            "    #[ignore = \"blocked\"]",        // 6 (f) reason-bearing -> no entry
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
            (10, Kind::Untracked, "todo!(\"later\")".to_string()),
        ];
        assert_eq!(got, expected);
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
            "    #[ignore = \"see #42\"]", // 4 reason-bearing ignore -> no entry (deferred to γ)
            "// TODO: wire this",         // 5 marker, no cite -> Structural(Untracked)
            "// TODO(#5): x  // ptodo:allow", // 6 inline escape on a cited line -> skipped
        ];
        let content = lines.join("\n");

        let got = scan_file(&content, true);
        let expected: Vec<(usize, LineClass, String)> = vec![
            (1, LineClass::Cited(vec![4553]), "// TODO(#4553): x".to_string()),
            (3, LineClass::Cited(vec![42]), "todo!()".to_string()),
            (5, LineClass::Structural(Kind::Untracked), "// TODO: wire this".to_string()),
        ];
        assert_eq!(got, expected);

        // Regression: classify_file is exactly scan_file filtered to its
        // Structural variants — the Cited markers (1, 3) and the suppressed
        // lines (2, 4, 6) drop out, leaving byte-identical α output.
        let classified = classify_file(&content, true);
        let expected_structural: Vec<(usize, Kind, String)> =
            vec![(5, Kind::Untracked, "// TODO: wire this".to_string())];
        assert_eq!(classified, expected_structural);
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
}
