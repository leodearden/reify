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

// -----------------------------------------------------------------------
// §8.1 marker recognition (pure, hand-rolled — no `regex` dep per design §12)
// -----------------------------------------------------------------------

/// `true` when `b` is an ASCII word byte (`[A-Za-z0-9_]`) — the alphabet for
/// the hand-rolled `\b` word-boundary checks in [`find_comment_marker`].
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
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
/// the `.rs`-only gating lives in [`classify_file`].
fn find_macro_stub(line: &str) -> bool {
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
        if is_task {
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
