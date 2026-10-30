//! In-memory answers about one branch's history, parsed from two `git log`
//! dumps of one tip commit.
//!
//! Pure: this module runs no git. Each dump's argv lives beside the parser
//! that reads it, so the wire format and its reader cannot drift apart; the
//! caller runs the argv and hands the output back. Both dumps pass
//! `--no-show-signature`, so a `log.showSignature` setting cannot write
//! verification lines into them.
//!
//! Every query answers only what the dump can answer exactly as the
//! equivalent per-call git command would, and returns `None` otherwise, so a
//! caller falls back to that command for anything else.

use std::collections::{HashMap, HashSet};

/// True iff `s` is a full lowercase hexadecimal SHA-1 object name.
fn is_full_sha(s: &str) -> bool {
    s.len() == 40
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The `git` argv whose stdout [`parse_tip`] reads: the commit `branch` names
/// now. Resolved once, so every dump of a snapshot reads the same history.
pub(crate) fn tip_argv(branch: &str) -> Vec<String> {
    vec![
        "rev-parse".to_string(),
        "--verify".to_string(),
        format!("{branch}^{{commit}}"),
    ]
}

/// Parse the stdout of [`tip_argv`]: one full SHA and a newline.
pub(crate) fn parse_tip(raw: &[u8]) -> Result<String, String> {
    std::str::from_utf8(raw)
        .ok()
        .and_then(|s| s.strip_suffix('\n'))
        .filter(|sha| is_full_sha(sha))
        .map(str::to_string)
        .ok_or_else(|| {
            format!(
                "the tip is not one full SHA: {:?}",
                String::from_utf8_lossy(raw)
            )
        })
}

/// One commit of [`BranchMessages`].
struct CommitMessage {
    sha: String,
    subject: String,
    /// The subject was not valid UTF-8 and `subject` holds its lossy decoding.
    /// Per-call `git log --grep` fails outright on such output, so a grep that
    /// lists this commit is declined rather than answered differently.
    subject_lossy: bool,
    /// The full message (`%B`: subject and body), lossily decoded. An ASCII
    /// substring is found in the lossy text exactly where it is in the bytes.
    message: String,
}

/// Every commit reachable from a branch, with its subject and full message, in
/// `git log` order.
pub(crate) struct BranchMessages {
    commits: Vec<CommitMessage>,
    shas: HashSet<String>,
}

impl BranchMessages {
    /// The `git` argv whose stdout [`BranchMessages::parse`] reads: every
    /// commit reachable from `tip`.
    pub(crate) fn argv(tip: &str) -> Vec<String> {
        vec![
            "log".to_string(),
            tip.to_string(),
            "--no-show-signature".to_string(),
            "-z".to_string(),
            "--format=%H%x00%s%x00%B".to_string(),
        ]
    }

    /// Parse the stdout of [`BranchMessages::argv`]: NUL-separated
    /// `(sha, subject, message)` triples, each commit's record closed by a NUL
    /// terminator. A commit message cannot contain NUL, so the layout is
    /// unambiguous; anything that does not fit it is refused.
    pub(crate) fn parse(raw: &[u8]) -> Result<Self, String> {
        let mut fields: Vec<&[u8]> = raw.split(|&b| b == 0).collect();
        if fields.pop() != Some(&[][..]) {
            return Err("the message dump does not end with a NUL terminator".to_string());
        }
        if !fields.len().is_multiple_of(3) {
            return Err(format!(
                "the message dump has {} fields, not a whole number of (sha, subject, message) triples",
                fields.len()
            ));
        }
        let mut commits = Vec::with_capacity(fields.len() / 3);
        let mut shas = HashSet::with_capacity(fields.len() / 3);
        for triple in fields.chunks_exact(3) {
            let sha = std::str::from_utf8(triple[0])
                .ok()
                .filter(|s| is_full_sha(s))
                .ok_or_else(|| {
                    format!(
                        "the message dump has a malformed commit name {:?}",
                        String::from_utf8_lossy(triple[0])
                    )
                })?;
            let (subject, subject_lossy) = match std::str::from_utf8(triple[1]) {
                Ok(s) => (s.to_string(), false),
                Err(_) => (String::from_utf8_lossy(triple[1]).into_owned(), true),
            };
            shas.insert(sha.to_string());
            commits.push(CommitMessage {
                sha: sha.to_string(),
                subject,
                subject_lossy,
                message: String::from_utf8_lossy(triple[2]).into_owned(),
            });
        }
        Ok(Self { commits, shas })
    }

    /// True iff `sha` is the full name of a commit reachable from the branch.
    /// `false` means only "not held here": an abbreviation or a ref name is
    /// never held, whatever it names.
    pub(crate) fn contains(&self, sha: &str) -> bool {
        self.shas.contains(sha)
    }

    /// `git log <branch> --grep=<pattern>` as `(sha, subject)` pairs in
    /// `git log` order, or `None` when that cannot be answered exactly here.
    ///
    /// Answered only for a non-empty, all-ASCII-alphanumeric pattern: such a
    /// pattern is a literal under BRE, ERE and PCRE alike, and `--grep` matches
    /// it against the message lines, so a substring test of the message is the
    /// same question. A custom `--format` searches no notes. Declined too when
    /// a matching commit's subject is not valid UTF-8, where the per-call
    /// command fails rather than answers.
    pub(crate) fn grep_literal(&self, pattern: &str) -> Option<Vec<(&str, &str)>> {
        if pattern.is_empty() || !pattern.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return None;
        }
        let mut hits = Vec::new();
        for commit in self.commits.iter().filter(|c| c.message.contains(pattern)) {
            if commit.subject_lossy {
                return None;
            }
            hits.push((commit.sha.as_str(), commit.subject.as_str()));
        }
        Some(hits)
    }
}

/// The paths each commit reachable from a branch changed against its first
/// parent, as `git diff --name-only --no-renames <c>^1..<c>` lists them.
pub(crate) struct FirstParentChanges {
    paths: HashMap<String, Vec<String>>,
}

impl FirstParentChanges {
    /// The `git` argv whose stdout [`FirstParentChanges::parse`] reads: every
    /// commit reachable from `tip`.
    ///
    /// Deliberately without `-z`: without it git C-quotes any path holding a
    /// TAB or a newline, so a line that starts with TAB can only be a header,
    /// and every path comes out quoted exactly as `git diff --name-only`
    /// prints it.
    pub(crate) fn argv(tip: &str) -> Vec<String> {
        vec![
            "log".to_string(),
            tip.to_string(),
            "--no-show-signature".to_string(),
            "--diff-merges=first-parent".to_string(),
            "--name-only".to_string(),
            "--no-renames".to_string(),
            "--format=%x09%H %P".to_string(),
        ]
    }

    /// Parse the stdout of [`FirstParentChanges::argv`]: per commit, a header
    /// line `TAB <sha> <parents…>`, then its changed paths one per line, with
    /// blank lines between. A path before any header is refused.
    ///
    /// A commit with no parent is not recorded: `<root>^1` names nothing, so
    /// the per-call command fails for it, and only that command can answer the
    /// way it does.
    pub(crate) fn parse(dump: &str) -> Result<Self, String> {
        let mut paths: HashMap<String, Vec<String>> = HashMap::new();
        // `None` before the first header; `Some(None)` inside a commit that is
        // not recorded.
        let mut current: Option<Option<String>> = None;
        for line in dump.lines() {
            if let Some(header) = line.strip_prefix('\t') {
                let mut names = header.split(' ');
                let sha = names.next().unwrap_or_default();
                if !is_full_sha(sha) {
                    return Err(format!("the path dump has a malformed header {line:?}"));
                }
                let has_parent = names.any(|p| !p.is_empty());
                if has_parent && paths.insert(sha.to_string(), Vec::new()).is_some() {
                    return Err(format!("the path dump lists commit {sha} twice"));
                }
                current = Some(has_parent.then(|| sha.to_string()));
            } else if !line.is_empty() {
                match &current {
                    None => return Err(format!("the path dump lists {line:?} before any commit")),
                    Some(None) => {}
                    Some(Some(sha)) => paths
                        .get_mut(sha)
                        .expect("a recorded header inserted its entry")
                        .push(line.to_string()),
                }
            }
        }
        Ok(Self { paths })
    }

    /// The paths `sha` changed against its first parent, or `None` when `sha`
    /// is not a full name of a commit with a parent reachable from the branch.
    pub(crate) fn changed_paths(&self, sha: &str) -> Option<&[String]> {
        self.paths.get(sha).map(Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const C: &str = "cccccccccccccccccccccccccccccccccccccccc";

    fn messages(records: &[(&str, &[u8], &[u8])]) -> Vec<u8> {
        let mut raw = Vec::new();
        for (sha, subject, message) in records {
            for field in [sha.as_bytes(), subject, message] {
                raw.extend_from_slice(field);
                raw.push(0);
            }
        }
        raw
    }

    #[test]
    fn grep_literal_matches_the_whole_message_in_log_order() {
        let raw = messages(&[
            (A, b"feat(123): y", b"feat(123): y\n"),
            (B, b"chore: z", b"chore: z\n\nRefs task 12.\n"),
            (C, b"docs", b"docs\n"),
        ]);
        let parsed = BranchMessages::parse(&raw).expect("well-formed dump");
        assert_eq!(
            parsed.grep_literal("12"),
            Some(vec![(A, "feat(123): y"), (B, "chore: z")]),
        );
        assert_eq!(parsed.grep_literal("999"), Some(vec![]));
        assert!(parsed.contains(B));
        assert!(!parsed.contains(&B[..7]), "an abbreviation is never held");
    }

    #[test]
    fn grep_literal_declines_what_it_cannot_answer_exactly() {
        let raw = messages(&[
            (A, b"feat(12): \xff", b"feat(12): \xff\n"),
            (B, b"x", b"x\n"),
        ]);
        let parsed = BranchMessages::parse(&raw).expect("invalid UTF-8 does not sink the dump");
        assert_eq!(parsed.grep_literal(""), None);
        assert_eq!(parsed.grep_literal("1.2"), None);
        assert_eq!(
            parsed.grep_literal("12"),
            None,
            "a lossy subject is never handed out"
        );
        assert_eq!(parsed.grep_literal("x"), Some(vec![(B, "x")]));
    }

    #[test]
    fn message_dump_accepts_an_empty_message_and_refuses_malformed_input() {
        let parsed = BranchMessages::parse(&messages(&[(A, b"", b"")])).expect("empty message");
        assert_eq!(
            parsed.grep_literal("a"),
            Some(vec![]),
            "the SHA is not searched"
        );

        let well_formed = messages(&[(A, b"s", b"s\n")]);
        assert!(BranchMessages::parse(&well_formed[..well_formed.len() - 1]).is_err());
        assert!(BranchMessages::parse(format!("{A}\0s\0").as_bytes()).is_err());
        assert!(BranchMessages::parse(&messages(&[("abc123", b"s", b"s\n")])).is_err());
        assert!(BranchMessages::parse(&messages(&[(&A.to_uppercase(), b"s", b"s\n")])).is_err());
        assert!(BranchMessages::parse(b"").is_ok_and(|m| m.grep_literal("a") == Some(vec![])));
    }

    #[test]
    fn path_dump_records_each_commit_with_a_parent() {
        let dump =
            format!("\t{A} {B} {C}\n\nf1.rs\n\"tab\\tname.rs\"\n\t{B} {C}\n\t{C} \n\nroot.rs\n");
        let parsed = FirstParentChanges::parse(&dump).expect("well-formed dump");
        assert_eq!(
            parsed.changed_paths(A),
            Some(&["f1.rs".to_string(), "\"tab\\tname.rs\"".to_string()][..]),
            "a quoted path is kept verbatim",
        );
        assert_eq!(
            parsed.changed_paths(B),
            Some(&[][..]),
            "a commit with no names"
        );
        assert_eq!(
            parsed.changed_paths(C),
            None,
            "a root commit is not recorded"
        );
        assert_eq!(parsed.changed_paths(&A[..7]), None);
    }

    #[test]
    fn tip_is_one_full_sha_and_a_newline() {
        assert_eq!(parse_tip(format!("{A}\n").as_bytes()), Ok(A.to_string()));
        assert!(parse_tip(A.as_bytes()).is_err());
        assert!(parse_tip(format!("{A}\n{B}\n").as_bytes()).is_err());
        assert!(parse_tip(b"abc123\n").is_err());
        assert!(parse_tip(b"").is_err());
    }

    #[test]
    fn path_dump_refuses_malformed_input() {
        assert!(FirstParentChanges::parse("orphan.rs\n").is_err());
        assert!(FirstParentChanges::parse("\tnot-a-sha deadbeef\n").is_err());
        assert!(FirstParentChanges::parse(&format!("\t{A} {B}\n\t{A} {B}\n")).is_err());
        assert!(FirstParentChanges::parse("").is_ok());
    }
}
