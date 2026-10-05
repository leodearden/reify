//! What the artifact header may honestly state about the git state it was
//! generated from — and the refusal when it cannot.

use crate::workspace_git::git_read;

// ─── step 17/18: a rebase-durable stamp, refused when it would be dishonest ──

/// A full git object name as `git rev-parse`/`git merge-base` print one:
/// exactly 40 lowercase hexadecimal characters.
pub(crate) const FULL_SHA_LEN: usize = 40;

/// What the artifact header states about the git state it was generated from.
///
/// Two kinds of fact, treated differently ON PURPOSE, and the discriminator is
/// REACHABILITY rather than severity:
///
/// * uncommitted bytes are reachable from NO commit, so no wording in a header
///   could let a reader reconstruct what was actually surveyed — a refusal is
///   the only way the snapshot claim stays honest. [`stamp_decision`] refuses,
///   and likewise refuses an anchor that is not a resolved object name, which
///   names nothing at all;
/// * drifted tracked `.ri` ARE committed and reachable from the surveyed
///   commit, so NAMING them in full makes the header honest without refusing.
///   They are disclosed here instead.
///
/// Refusing the drift case would also be unsatisfiable exactly where the
/// artifact expects to be re-run: its own header names γ as the task that will
/// legitimately invalidate it, and γ's diff IS `.ri` migrations — so the branch
/// chartered to regenerate the survey could never regenerate it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SurveyStamp {
    /// The commit the header names — `git merge-base main HEAD`; see
    /// [`survey_stamp`] for why never the branch tip.
    pub(crate) anchor: String,
    /// Tracked `.ri` differing between [`Self::anchor`] and the surveyed
    /// commit, verbatim from `git diff --name-only <anchor> HEAD -- '*.ri'`.
    ///
    /// Empty is the ordinary case and renders NOTHING, so an undrifted artifact
    /// carries no disclosure at all — no permanent "0 files drifted" row, and
    /// runs generated on `main` stay byte-comparable with each other.
    pub(crate) drifted: Vec<String>,
}

impl SurveyStamp {
    /// The undrifted shape: an anchor that describes the surveyed corpus exactly.
    pub(crate) fn at(anchor: &str) -> Self {
        Self {
            anchor: anchor.to_owned(),
            drifted: Vec::new(),
        }
    }
}

/// The lines of `drift` — raw `git diff --name-only` output — that name a
/// member of `corpus`, in git's own order and spelling.
///
/// The SINGLE parser of that output: [`stamp_decision`] takes the result of this
/// already split and already filtered, so nothing else has to know that git
/// writes a trailing newline even when it has nothing to report.
fn drift_within_corpus(drift: &str, corpus: &std::collections::BTreeSet<&str>) -> Vec<String> {
    drift
        .lines()
        .map(str::trim)
        .filter(|path| corpus.contains(path))
        .map(str::to_owned)
        .collect()
}

/// The union of both corpus halves, as the membership set
/// [`drift_within_corpus`] filters against.
fn surveyed_corpus_union<'a>(
    ri: &'a [String],
    hosts: &'a [String],
) -> std::collections::BTreeSet<&'a str> {
    ri.iter().chain(hosts).map(String::as_str).collect()
}

/// The drift disclosure names exactly the files whose bytes a ROW describes.
///
/// Widening the git read from `'*.ri'` to `'*.ri' '*.rs'` makes it see every
/// unrelated `.rs` churn in the repo — thousands of files no row mentions — so
/// the filter is what keeps the header a disclosure rather than a changelog.
#[test]
fn drift_within_corpus_keeps_both_halves_and_drops_everything_else() {
    let ri = vec!["examples/a.ri".to_owned(), "stdlib/b.ri".to_owned()];
    let hosts = vec!["crates/c/tests/h.rs".to_owned()];
    let corpus = surveyed_corpus_union(&ri, &hosts);

    let drift = "examples/a.ri\n\
                 crates/c/tests/h.rs\n\
                 crates/c/src/lib.rs\n\
                 docs/prds/unrelated.md\n\
                 \n";
    assert_eq!(
        drift_within_corpus(drift, &corpus),
        vec!["examples/a.ri".to_owned(), "crates/c/tests/h.rs".to_owned(),],
        "the disclosure must name BOTH halves' members and NOTHING else — a `.rs` \
         outside the host corpus is churn no row describes, and listing it would \
         flood the header"
    );
    assert!(
        drift_within_corpus("", &corpus).is_empty(),
        "git writes a trailing newline even with nothing to report"
    );
    assert!(
        drift_within_corpus("  \n\n", &corpus).is_empty(),
        "a whitespace-only read is an EMPTY read; this is the ONE place that \
         parses git's `--name-only` output, so nothing downstream re-derives it"
    );
}

/// Decide whether the git state just read may be stamped into the artifact
/// header — or whether stamping it would make that header lie.
///
/// Pure by construction, so the decision is gate-resident and unit-tested with
/// no git state at all; the three reads that feed it live in [`survey_stamp`],
/// behind the `#[ignore]`d generator. Same split this module uses throughout.
///
/// * `anchor` — `git merge-base main HEAD`, the commit the header will name.
/// * `dirty` — `git status --porcelain --untracked-files=no`, raw.
/// * `drifted` — the corpus members that differ between the anchor and `HEAD`,
///   already parsed out of `git diff --name-only` and already narrowed to the
///   two corpora by [`drift_within_corpus`].
///
/// The first two can REFUSE; `drifted` never does — it is disclosed. See
/// [`SurveyStamp`] for the reachability argument that splits them.
///
/// Whitespace-only `dirty` is an EMPTY read: git writes a trailing newline even
/// when it has nothing to report.
fn stamp_decision(anchor: &str, dirty: &str, drifted: &[String]) -> Result<SurveyStamp, String> {
    let anchor = anchor.trim();
    if anchor.len() != FULL_SHA_LEN || !anchor.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
        return Err(format!(
            "refusing to stamp {anchor:?}: the artifact header must name a fully \
             resolved commit ({FULL_SHA_LEN} lowercase hex characters), never an \
             abbreviated name or a symbolic ref that names a moving target"
        ));
    }

    let dirty = dirty.trim();
    if !dirty.is_empty() {
        return Err(format!(
            "refusing to stamp {anchor}: the working tree is dirty, so the \
             header's claim to be a snapshot AT that commit would be false — the \
             bytes surveyed are not the bytes at the stamped commit. Commit \
             first, then re-run the generator. Dirty:\n{dirty}"
        ));
    }

    Ok(SurveyStamp {
        anchor: anchor.to_owned(),
        drifted: drifted.to_vec(),
    })
}

/// A drift list as [`drift_within_corpus`] hands one to [`stamp_decision`].
#[cfg(test)]
fn drifted_paths(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|p| (*p).to_owned()).collect()
}

#[test]
fn stamp_decision_accepts_a_resolved_anchor_over_a_clean_tree() {
    // The one shape that may be stamped: a fully-resolved anchor, nothing
    // uncommitted, and no corpus member differing between the anchor and the
    // commit actually swept.
    let anchor = "a46387d1f58fb469ed226cc0f2bfbaafa7cf63be";
    assert_eq!(
        stamp_decision(anchor, "", &[]),
        Ok(SurveyStamp::at(anchor)),
        "a resolved anchor over a clean, undrifted tree is exactly what the \
         header is allowed to claim"
    );
    // git writes a trailing newline even when it has nothing to report, and a
    // whitespace-only read is an EMPTY read — not a refusal.
    assert_eq!(
        stamp_decision(anchor, "\n", &[]),
        Ok(SurveyStamp::at(anchor)),
        "whitespace-only git output means clean; it must not be read as dirty"
    );
}

#[test]
fn stamp_decision_refuses_a_dirty_tree_and_names_what_is_dirty() {
    // Why this refusal exists: the artifact header CLAIMS to be a snapshot at
    // the stamped commit. That claim is FALSE if the tree carried uncommitted
    // edits when the sweep ran — the bytes surveyed would not be the bytes at
    // the stamped commit. Refusing is the only way the header stays honest.
    //
    // The refusal must echo the offending `git status --porcelain` payload so
    // the operator can see WHICH files blocked the stamp. The remedy sentence
    // itself is deliberately NOT pinned here: asserting on its wording would
    // test the message rather than the behaviour.
    let anchor = "a46387d1f58fb469ed226cc0f2bfbaafa7cf63be";
    let err = stamp_decision(anchor, " M crates/reify-compiler/src/lib.rs\n", &[])
        .expect_err("a dirty tree must refuse to stamp");
    assert!(
        err.contains("crates/reify-compiler/src/lib.rs"),
        "the refusal must name the dirty path it read, so the operator can act \
         on it without re-running git by hand; got: {err}"
    );
    // A STAGED addition is still dirty — `--untracked-files=no` suppresses the
    // `??` rows only, never the `A `/` M` ones.
    assert!(
        stamp_decision(anchor, "A  docs/prds/new.md\n", &[]).is_err(),
        "a staged-but-uncommitted addition must refuse too"
    );
}

#[test]
fn stamp_decision_discloses_a_drifted_tracked_ri_rather_than_refusing() {
    // A tracked `.ri` differing between the anchor and the commit swept makes
    // the anchor an INCOMPLETE description of the surveyed corpus — not an
    // unrecoverable one. The drifted bytes are COMMITTED and reachable from the
    // surveyed commit, so naming every one of them makes the header fully
    // honest; a refusal would additionally be unsatisfiable on the one branch
    // this artifact names as its expected invalidator, whose whole diff is
    // `.ri` migrations.
    let anchor = "a46387d1f58fb469ed226cc0f2bfbaafa7cf63be";
    let stamp = stamp_decision(anchor, "", &drifted_paths(&["examples/one.ri", "examples/two.ri"]))
        .expect("a committed .ri drift is disclosed, never refused");
    assert_eq!(
        stamp.anchor, anchor,
        "drift must not promote the surveyed tip: the merge base stays THE \
         anchor, because a branch tip is rewritable and this one is not"
    );
    assert_eq!(
        stamp.drifted,
        vec!["examples/one.ri".to_owned(), "examples/two.ri".to_owned()],
        "every drifted path git reported must survive into the disclosure, in \
         git's own order and spelling — the disclosure is machine-generated, so \
         a path it drops is a path no reader can know about"
    );
}

#[test]
fn stamp_decision_still_refuses_an_unreachable_state_even_alongside_drift() {
    // The discriminator between refusing and disclosing is REACHABILITY, never
    // "how much changed" — so drift, which is disclosable, must not soften
    // either refusal it travels with. Uncommitted bytes are reachable from no
    // commit and an unresolved anchor names no commit at all; in both cases no
    // wording in the header could let a reader reconstruct what was surveyed.
    let anchor = "a46387d1f58fb469ed226cc0f2bfbaafa7cf63be";
    let err = stamp_decision(anchor, " M docs/prds/x.md\n", &drifted_paths(&["examples/one.ri"]))
        .expect_err("a dirty tree refuses whether or not a tracked .ri drifted");
    assert!(
        err.contains("docs/prds/x.md"),
        "the refusal must still name what is dirty; got: {err}"
    );
    assert!(
        stamp_decision("HEAD", "", &drifted_paths(&["examples/one.ri"])).is_err(),
        "an anchor that is not a resolved object name refuses whether or not a \
         tracked .ri drifted"
    );
}

#[test]
fn stamp_decision_rejects_an_anchor_that_is_not_a_full_lowercase_sha() {
    // Anything but a resolved 40-lowercase-hex object name is garbage in the
    // header: a failed/empty git read, an abbreviated name that a future repo
    // could render ambiguous, or a symbolic ref that names a MOVING target
    // rather than the commit surveyed.
    let bad_anchors = [
        ("", "an empty read — git produced nothing"),
        ("a46387d1f5", "an abbreviated name, not a full object name"),
        ("ref: refs/heads/main", "a symbolic ref names a moving target"),
        (
            "A46387D1F58FB469ED226CC0F2BFBAAFA7CF63BE",
            "uppercase — git never prints an object name this way",
        ),
        (
            "a46387d1f58fb469ed226cc0f2bfbaafa7cf63bz",
            "40 characters but not hexadecimal",
        ),
        (
            "a46387d1f58fb469ed226cc0f2bfbaafa7cf63bee",
            "41 characters — one too many",
        ),
    ];
    for (anchor, why) in bad_anchors {
        assert!(
            stamp_decision(anchor, "", &[]).is_err(),
            "{anchor:?} must be rejected rather than stamped: {why}"
        );
    }
    // Sanity: the guard rejects for the RIGHT reason — the same inputs with a
    // well-formed anchor are accepted.
    assert!(
        stamp_decision(&"a".repeat(FULL_SHA_LEN), "", &[]).is_ok(),
        "40 lowercase hex characters is the accepted shape"
    );
}

/// Read the git state the header will state: the commit stamped as the anchor
/// — `git merge-base main HEAD`, deliberately NOT `git rev-parse HEAD` — plus
/// any tracked `.ri` that has drifted from it.
///
/// A task-branch tip is a commit the merge machinery can, and demonstrably did,
/// rewrite. The first committed survey stamped `a0d0899874…`, the pre-rebase
/// duplicate of `23d1af852a` (identical subject, different tree), orphaned into
/// a dangling object when this branch was rebased onto a newer `main`. Stamping
/// another branch tip would merely re-arm the identical failure at the next
/// rebase, so this is the root fix rather than a re-stamp.
///
/// The merge base is ON `main`, and CLAUDE.md forbids moving `main` by any
/// history-rewriting route (no `git update-ref` / `reset` / `commit-tree` /
/// `merge --no-verify`), so it stays reachable permanently. It also survives
/// further rebases of THIS branch: rebasing onto a newer `main` keeps the older
/// `main` commit in ancestry, so the anchor remains an ancestor either way. And
/// it is the semantically correct reading of the header's own words — "enumerate
/// and size, once, at the base commit stamped above".
///
/// Panics rather than stamping any state that would make that header dishonest;
/// the decision itself is [`stamp_decision`], which is pure and gate-resident.
/// Drifted `.ri` are the one git fact that does NOT panic — they are committed,
/// so the header discloses them by name instead ([`SurveyStamp`]).
///
/// For the reader: only THIS function — which runs solely inside the
/// `#[ignore]`d generator — depends on a `main` ref existing. The gate-resident
/// guard `committed_survey_stamps_a_commit_that_is_an_ancestor_of_head`
/// deliberately does not, so a checkout without `main` cannot red the gate.
pub(crate) fn survey_stamp(ri: &[String], hosts: &[String]) -> SurveyStamp {
    let anchor = git_read(&["merge-base", "main", "HEAD"]);
    // `--untracked-files=no` is deliberate: `git ls-files` never surfaces an
    // untracked file, so an untracked scratch file cannot change one row of the
    // survey and must not trigger a spurious refusal. A staged addition still
    // appears as `A ` and is still caught.
    let dirty = git_read(&["status", "--porcelain", "--untracked-files=no"]);
    // Both extensions, because a row can now describe the bytes of either half.
    // The read is therefore far wider than the disclosure: every unrelated `.rs`
    // churn in the repo lands in it, which is why the result is narrowed to the
    // two corpora before it reaches the header.
    let drift = git_read(&["diff", "--name-only", &anchor, "HEAD", "--", "*.ri", "*.rs"]);
    let drifted = drift_within_corpus(&drift, &surveyed_corpus_union(ri, hosts));
    stamp_decision(&anchor, &dirty, &drifted)
        .unwrap_or_else(|e| panic!("ctor_conformance_corpus_survey: {e}"))
}
