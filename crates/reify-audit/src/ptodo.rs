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
}
