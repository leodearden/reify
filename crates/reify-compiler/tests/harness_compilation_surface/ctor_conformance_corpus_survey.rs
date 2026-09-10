//! Struct-ctor field-type conformance — corpus survey generator (task #5304).
//!
//! PRD `docs/prds/struct-ctor-field-type-conformance.md`, task β (§8): run the
//! α(+ε) warn-stage compiler over **all tracked `.ri`** and commit
//! `docs/prds/struct-ctor-field-type-conformance.survey.md` — every warning
//! site, classified per D9, with the regeneration command. The signal is
//! "mechanized, not a hand audit": every row and every count in that artifact
//! is produced by the code in this module, with zero hand-derived entries.
//!
//! # Why this lives HERE and not in a new `tests/*.rs` binary
//!
//! `tests/infra/test_harness_kloc_cap.sh` rule (b) flags any NEW standalone
//! top-level `crates/reify-compiler/tests/*.rs` as `reason=unsanctioned-standalone`
//! unless a grandfather-baseline row is added — an explicitly-discouraged
//! "conscious baseline edit" whose whole point is to stop new test binaries
//! silently re-accreting against the merge-gate link count
//! (`docs/prds/merge-gate-compile-cost.md` §5 C1). Folding the generator into
//! this already-consolidated unit adds no link at all, and the unit is
//! thematically exact — "what you hand the compiler … examples". The sibling
//! `examples_smoke.rs` already runs the identical parse→compile→filter pipeline
//! over `examples/`; β widens the root to the whole tracked corpus.
//!
//! # Why the expensive walk is `#[ignore]`d and the decisions are not
//!
//! Compiling the ~261 `examples/` files is documented as "the single most
//! expensive thing this binary does" (`examples_smoke.rs`); the ~660 tracked
//! files are ~2.5× that, and paying it on every merge gate would directly fight the
//! merge-gate-compile-cost PRD. So the full corpus walk is ONE `#[ignore]`d
//! generator, run on demand — while everything it *decides* (corpus
//! enumeration, span→line, ctor-name recovery, field/expected/found
//! extraction, D9 classification, markdown rendering) is factored into pure
//! helpers that ARE gate-resident and unit-tested here against synthetic
//! inputs, plus one cheap end-to-end sweep over a 3-file synthetic corpus.
//! The pipeline is therefore regression-guarded on every gate run at near-zero
//! cost, without the walk itself ever running there.
//!
//! # Retiring this module
//!
//! This is a CENSUS, not a permanent gate, and it has a defined end of life.
//! Its product is one 280-line document with 18 rows, consumed by task #5305
//! (γ, corpus fix-forward). Once γ has landed, the machinery here — corpus
//! enumeration, span→line, D9 classification, the markdown renderer, the stamp
//! guard — has no remaining product, yet stays compiled and run on every merge
//! gate. That is a real standing cost in a compile unit whose own header cites
//! `docs/prds/merge-gate-compile-cost.md`: it takes this unit to 14,629 lines
//! against the 20,000 `CAP_LINES` in `tests/infra/test_harness_kloc_cap.sh`
//! (raw `wc -l` summed over the root and its `#[path]` members, which is how
//! rule (a) there measures — re-measured on this branch, not carried over).
//!
//! Retirement is therefore a THREE-FILE deletion, and all three must go
//! together:
//!
//! 1. this file;
//! 2. its `#[path] mod ctor_conformance_corpus_survey;` declaration in
//!    `crates/reify-compiler/tests/harness_compilation_surface.rs`;
//! 3. the artifact `docs/prds/struct-ctor-field-type-conformance.survey.md`.
//!
//! The ONE thing that must survive is [`CTOR_CONFORMANCE_CODES`], which the
//! sibling `examples_smoke.rs` α corpus gate reads and which outlives this
//! survey — move it (see its own doc comment, which records where it wants to
//! land) rather than deleting it with the rest.
//!
//! Nothing here fails when the artifact is deleted on its own:
//! [`committed_survey_stamps_a_commit_that_is_an_ancestor_of_head`] SKIPS on an
//! absent artifact by design, so a partial retirement degrades to dead weight
//! rather than a merge-gate red.

use std::path::PathBuf;
use std::process::Command;

// The workspace's SINGLE definition of the repo-redirect sanitizer, not a local
// twin: `sanitize` is what `git_at_workspace_root` below applies, and
// `REPO_REDIRECT_VARS`/`removed_vars` are what its contract tests read, so this
// module cannot drift from the set it is supposed to remove.
// (`crates/reify-test-support/src/git_env.rs`.)
use reify_test_support::git_env::{REPO_REDIRECT_VARS, removed_vars, sanitize};

/// Absolute path to the workspace root, resolved at compile time from this
/// crate's manifest directory (two levels up).
///
/// Same rooting idiom as `examples_smoke.rs`'s `EXAMPLES_DIR`, pointed one
/// level higher: β's whole point is that the sweep is NOT examples-scoped.
const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

// ─── step 21/22: one sanitized git constructor for every call site ───────────

/// A pre-sanitized `git -C <workspace root>` command — the ONLY way this
/// module reaches git.
///
/// Every git call here targets a specific repository (this workspace), and the
/// workspace rule for that shape is stated at `reify_audit::git_env`: build it
/// through a sanitized `-C <root>` constructor, because git exports
/// `GIT_DIR`/`GIT_INDEX_FILE`/`GIT_WORK_TREE` into a hook's entire process tree
/// and those OVERRIDE an explicit `-C`. The failure is silent — the command
/// operates on a *different* repository rather than erroring — which for this
/// module would mean enumerating some other tree's `.ri` files into the survey,
/// or resolving the stamped anchor against the wrong object store. The rule's
/// one carve-out is a bare `git --version` availability probe — which is
/// exactly [`git_is_available`] below, and the ONLY spawn in this module that
/// does not come from here; every repo-targeting call site routes through this
/// constructor.
///
/// `reify_audit::git_env::command` is the same shape one crate up, and is
/// deliberately NOT used: `reify-audit` is not a dependency of
/// `reify-compiler`, and adding that edge to reuse a four-line constructor
/// would be a far larger change than the sanctioned below-the-edge pattern its
/// own doc describes — route repo-targeting spawns through
/// `reify_test_support::git_env::sanitize` directly, which is the same single
/// sanitizer either way.
///
/// Returns an owned `Command` rather than borrowing `sanitize`'s `&mut` return,
/// because two of the three consumers need `.output()` and one needs only
/// `.status`.
fn git_at_workspace_root(args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(WORKSPACE_ROOT);
    cmd.args(args);
    sanitize(&mut cmd);
    cmd
}

/// Whether a `git` binary can be spawned at all.
///
/// The sanctioned carve-out from the `-C`-constructor rule above: a bare
/// `git --version` names no repository, so there is nothing for an ambient
/// `GIT_DIR` to redirect and nothing to sanitize against.
///
/// Exists so the GATE-RESIDENT tests can SKIP rather than red when git is
/// absent. Everything in this module that reads git — the corpus enumeration
/// and the stamp guard — is a survey concern, not a compiler concern, and
/// `reify-compiler`'s test suite did not require git on PATH before this module
/// existed. A survey generator must not be the thing that makes it a hard
/// requirement. Distinguishing "git said no" (a real finding, still a hard
/// failure) from "there is no git" (nothing to say) is the whole point: the
/// panics in [`git_read`] / [`git_succeeds`] / [`scan_tracked_ri_corpus`] stay
/// exactly as they were for every caller that gets past this probe.
fn git_is_available() -> bool {
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        Command::new("git")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

/// `Some(corpus)` when git can be spawned, `None` when it cannot.
///
/// The gate-resident corpus probes below call this and return early on `None`;
/// the `#[ignore]`d generator calls [`tracked_ri_corpus`] directly, because a
/// generator that cannot enumerate the corpus must fail loudly rather than
/// write a falsely-clean artifact.
fn tracked_ri_corpus_if_git_available() -> Option<&'static [String]> {
    git_is_available().then(tracked_ri_corpus)
}

#[test]
fn git_at_workspace_root_removes_every_repo_redirect_var() {
    // Iterates the SHARED constant rather than naming the vars locally, so a
    // future GROWTH of the sanitized set is covered here with no edit to this
    // file. Same shape as `reify_audit::git_env`'s own wiring test
    // (`command_removes_every_repo_redirect_var`), which likewise reads the
    // `(key, None)` removal encoding through the definition site's
    // `removed_vars` rather than a hand-copied local twin.
    //
    // What this deliberately does NOT re-prove: that the removals actually
    // defeat a real ambient redirect var, and that an unsanitized `-C` loses to
    // one. That is spawned against real git at the definition site, in
    // `reify_test_support::git_env`'s
    // `sanitize_makes_dash_c_authoritative_against_real_git`. Re-spawning git
    // here would buy no new signal and would add a git-availability skip path.
    let cmd = git_at_workspace_root(&["rev-parse", "HEAD"]);
    let removed = removed_vars(&cmd);
    for var in REPO_REDIRECT_VARS {
        assert!(
            removed.iter().any(|r| r == var),
            "git_at_workspace_root must REMOVE `{var}` (env_remove -> `(key, None)`), \
             not merely overwrite it; removals seen: {removed:?}"
        );
    }
}

#[test]
fn git_at_workspace_root_targets_git_dash_c_at_the_workspace_root() {
    // The other half of the guarantee, and the half sanitization alone cannot
    // give: a refactor could keep every `env_remove` in place while dropping
    // the `-C`. Every git call in this module runs from the crate directory,
    // never the repo root, so the `-C` is what makes the command name THIS
    // repository at all — and an exact, ordered argv comparison is what pins
    // that the caller's args follow the prefix rather than replace it.
    let cmd = git_at_workspace_root(&["rev-parse", "HEAD"]);

    assert_eq!(cmd.get_program(), std::ffi::OsStr::new("git"));

    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        ["-C", WORKSPACE_ROOT, "rev-parse", "HEAD"],
        "expected exactly `-C <workspace root>` followed by the caller's args, in that order"
    );
}

// ─── step 1/2: corpus enumeration ────────────────────────────────────────────

/// Every TRACKED `.ri` file in the repository, as repo-relative
/// forward-slash paths, sorted and deduplicated.
///
/// Shells out to `git ls-files -z -- '*.ri'` at the workspace root rather than
/// walking the filesystem, for three reasons:
///
/// 1. The task defines the corpus as "all **tracked** `.ri`", and both the PRD
///    and the capability manifest cite `git ls-files '*.ri'` as the enumerating
///    command — so the survey's denominator is identical to the one the PRD
///    gate reasons about.
/// 2. A filesystem walk would have to exclude `target/` and every other
///    gitignored tree by hand, and would drift from that definition; a
///    build-artifact `.ri` could silently enter the survey.
/// 3. `-z` / NUL splitting means a path containing a space or a newline cannot
///    corrupt the list.
///
/// Panics if git is unavailable or exits non-zero. A silently-empty corpus
/// would render a falsely-clean survey, which is the one failure mode this
/// artifact must never have.
///
/// Enumerated ONCE per process, behind the same `OnceLock` that
/// [`stdlib_structure_defs`] and [`fea_owned_defs`] use: the tracked corpus
/// cannot change while the test binary runs, and the five gate-resident tests
/// below plus the generator would otherwise spawn six separate
/// `git ls-files` subprocesses and re-sort ~676 paths each time.
fn tracked_ri_corpus() -> &'static [String] {
    static CORPUS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    CORPUS.get_or_init(scan_tracked_ri_corpus)
}

/// The uncached enumeration behind [`tracked_ri_corpus`].
fn scan_tracked_ri_corpus() -> Vec<String> {
    let out = git_at_workspace_root(&["ls-files", "-z", "--", "*.ri"])
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "ctor_conformance_corpus_survey: cannot run `git ls-files` in {WORKSPACE_ROOT}: {e}"
            )
        });
    assert!(
        out.status.success(),
        "ctor_conformance_corpus_survey: `git ls-files -z -- '*.ri'` in {WORKSPACE_ROOT} \
         exited {:?}: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).trim()
    );

    let stdout = String::from_utf8(out.stdout)
        .expect("ctor_conformance_corpus_survey: `git ls-files` emitted non-UTF-8 paths");
    let mut paths: Vec<String> = stdout
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    paths.sort();
    paths.dedup();
    assert!(
        !paths.is_empty(),
        "ctor_conformance_corpus_survey: `git ls-files -z -- '*.ri'` returned nothing in \
         {WORKSPACE_ROOT} — a silently-empty corpus would render a falsely-clean survey"
    );
    paths
}

#[test]
fn tracked_ri_corpus_clears_the_broken_enumeration_floor() {
    let Some(corpus) = tracked_ri_corpus_if_git_available() else {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    };
    // A BROKEN-ENUMERATION floor, deliberately far below the live count (677
    // measured 2026-09-01) rather than just under it. The corpus is expected to
    // churn in BOTH directions: a fixture-consolidation task that legitimately
    // deletes a few dozen `.ri` has nothing to do with this survey and must not
    // red the merge gate with a message that reads like a defect.
    //
    // The NAME is scoped to exactly that floor and no further. An earlier name
    // ("…is_non_empty_and_covers_the_whole_tracked_tree") also claimed the
    // COVERAGE half, which is asserted in a different test entirely
    // (`tracked_ri_corpus_reaches_outside_examples`) — so a reader scanning the
    // test list, or triaging a red, was told this test proved something it did
    // not. The test name is what shows up in `cargo test` output; coverage
    // ownership stays with the test that actually asserts it. The live count
    // belongs in the artifact this module generates, which states it as a
    // measured header field.
    assert!(
        corpus.len() >= 100,
        "tracked .ri corpus must have >= 100 entries — a floor that catches a BROKEN \
         enumeration (wrong root, wrong pathspec, silent git failure), not a legitimate \
         shrink; the artifact header carries the live count. Got {}",
        corpus.len()
    );
}

#[test]
fn tracked_ri_corpus_entries_all_end_in_dot_ri() {
    let Some(corpus) = tracked_ri_corpus_if_git_available() else {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    };
    let bad: Vec<&String> = corpus.iter().filter(|p| !p.ends_with(".ri")).collect();
    assert!(
        bad.is_empty(),
        "every corpus entry must end in '.ri', got {} that do not: {:?}",
        bad.len(),
        &bad[..bad.len().min(5)]
    );
}

#[test]
fn tracked_ri_corpus_is_sorted_and_deduplicated() {
    // Determinism: the artifact must be byte-reproducible, which requires the
    // enumeration itself to be a total order with no repeats.
    let Some(corpus) = tracked_ri_corpus_if_git_available() else {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    };
    let mut expected = corpus.to_vec();
    expected.sort();
    expected.dedup();
    assert_eq!(
        corpus,
        expected.as_slice(),
        "tracked_ri_corpus must return a sorted, deduplicated list"
    );
}

// DELIBERATELY ABSENT: a gate-resident "every corpus entry resolves to an
// existing file on disk" probe.
//
// That is a property of the WORKING TREE, not of anything this module decides.
// `git ls-files` reports the INDEX, so an engineer who has `rm`'d a tracked
// `.ri` locally, or is mid-`git mv`, without staging the deletion would get a
// red `reify-compiler` suite pointing at the survey module with no connection to
// what they were doing. The three probes that remain
// (`…_all_end_in_dot_ri`, `…_is_sorted_and_deduplicated`,
// `…_paths_are_repo_relative_forward_slash`) are pure properties of the
// enumeration and carry no such coupling.
//
// The behaviour that actually matters when a member is missing is that the
// sweep RECORDS it rather than dying, and that IS gate-resident: see
// `survey_corpus_records_a_read_error_rather_than_panicking` and
// `survey_corpus_records_unsurveyable_members_instead_of_dropping_them`. Each
// unreadable member lands in `SurveyRun::not_surveyed` with reason
// `read-error`, and the rendered artifact lists it by name — which is the
// honest disclosure a stale tree deserves, not a merge-gate red.

#[test]
fn tracked_ri_corpus_reaches_outside_examples() {
    // The landed `discover_ri_files()` walk is rooted at `examples/` and would
    // miss ~399 of the ~660 tracked files. Widening the root IS β.
    let Some(corpus) = tracked_ri_corpus_if_git_available() else {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    };
    assert!(
        corpus
            .iter()
            .any(|p| p.starts_with("crates/reify-compiler/stdlib/")),
        "corpus must include stdlib members (β is not examples-scoped); \
         first 5 entries: {:?}",
        &corpus[..corpus.len().min(5)]
    );
    assert!(
        corpus.iter().any(|p| p.starts_with("examples/")),
        "corpus must still include the examples/ tree"
    );
    let non_examples = corpus
        .iter()
        .filter(|p| !p.starts_with("examples/"))
        .count();
    // Same reasoning as the corpus floor above: the load-bearing assertions are
    // the two structural `starts_with` probes, which hold at any size. This
    // number only has to be large enough to catch an enumeration that collapsed
    // back to the examples-scoped walk β exists to widen (399 measured at plan
    // time), and small enough that a legitimate fixture cull is not a merge-gate
    // red.
    assert!(
        non_examples >= 50,
        "the non-examples half is the point of β — a collapse back to the \
         examples-scoped walk must red, a legitimate fixture cull must not \
         (399 measured at plan time), got {non_examples}"
    );
}

#[test]
fn tracked_ri_corpus_paths_are_repo_relative_forward_slash() {
    let Some(corpus) = tracked_ri_corpus_if_git_available() else {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    };
    for p in corpus {
        assert!(
            !p.starts_with('/') && !p.starts_with("./") && !p.contains('\\'),
            "corpus entries must be repo-relative forward-slash paths, got {p:?}"
        );
    }
}

// ─── step 3/4: source-position helpers ───────────────────────────────────────

/// 1-based line of `span`'s START offset within `source`.
///
/// Delegates to `reify_core::byte_offset_to_line_col` rather than hand-rolling
/// newline counting — that helper is already multi-byte-correct and carries its
/// own round-trip tests, and it short-circuits the prelude sentinel to `(1, 1)`
/// in both debug and release builds.
///
/// The one thing added here is the OUT-OF-RANGE CLAMP: `byte_offset_to_line_col`
/// carries a `debug_assert!(offset <= source.len())`, so a synthetic or stale
/// span would abort a debug-profile sweep of 660 files.
///
/// The clamp bounds the resulting LINE, not merely the offset, and that
/// distinction is load-bearing for the artifact. Clamping the offset alone to
/// `source.len()` reports line 3 for a two-line file that ends in a newline —
/// `byte_offset_to_line_col` counts the phantom empty line after the trailing
/// `\n`. Every row in the survey is a `file:line` a human will open, so a line
/// number past the end of the file is a dangling pointer. The postcondition is
/// therefore `1 <= result <= source.lines().count().max(1)`: every emitted line
/// resolves to a real line of the swept file.
///
/// The prelude sentinel is deliberately NOT offset-clamped: it is passed
/// through so the callee's own `(1, 1)` short-circuit applies.
fn line_of_span(source: &str, span: reify_core::SourceSpan) -> u32 {
    let raw = span.start as usize;
    let offset = if raw == reify_core::SourceSpan::PRELUDE_SENTINEL_OFFSET {
        raw
    } else {
        raw.min(source.len())
    };
    let line = reify_core::byte_offset_to_line_col(source, offset).0 as u32;
    // `lines()` does not yield a trailing empty line for a source ending in
    // `\n`, which is exactly the bound wanted here. `.max(1)` keeps the empty
    // source reporting line 1 rather than 0.
    let last_line = source.lines().count().max(1) as u32;
    line.clamp(1, last_line)
}

/// Where a row's `def` came from — or, when it is absent, WHY.
///
/// Recorded PER ROW so the artifact never has to *assert* a cause in prose. An
/// earlier draft of the Unknown-group blurb claimed those rows "come through the
/// sub `=` per-arg anchor" — a hand-derived cause, and the failure mode this enum
/// exists to remove: several distinct shapes reach the same unattributed group,
/// and which one a given row took is knowable only where the recovery actually
/// ran. So no prose here restates a per-row cause; the artifact's `def source`
/// column carries the machine-derived arm, one row at a time. The arm docs
/// below describe only what each arm MATCHES — never which rows are in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DefOrigin {
    /// Recovered from the ctor call-site span anchor (α's expression path).
    CallSiteAnchor,
    /// Recovered from ε prose (`in call to '<Def>'` / `<Def>() expects …`).
    DiagnosticProse,
    /// The diagnostic carried no label at all, so there was no span to read.
    NoLabel,
    /// The label span points past the end of the file, or is the prelude
    /// sentinel — no source text exists at it.
    SpanOutOfRange,
    /// The label span starts inside a multi-byte codepoint; identifiers are
    /// ASCII-led, so this can never be a ctor anchor.
    SpanMidCodepoint,
    /// The span starts at something that is not an identifier — a literal, an
    /// operator, a delimiter.
    SpanNotIdentifier,
    /// An identifier was found but is not followed by `(`, so it is a plain
    /// reference rather than a call.
    IdentifierNotACall,
}

impl DefOrigin {
    /// Stable cell text for the artifact's `def source` column.
    fn label(self) -> &'static str {
        match self {
            DefOrigin::CallSiteAnchor => "ctor call-site anchor",
            DefOrigin::DiagnosticProse => "diagnostic prose",
            DefOrigin::NoLabel => "unrecovered: diagnostic carries no label",
            DefOrigin::SpanOutOfRange => "unrecovered: label span out of range",
            DefOrigin::SpanMidCodepoint => "unrecovered: label span mid-codepoint",
            DefOrigin::SpanNotIdentifier => "unrecovered: label span starts at a non-identifier",
            DefOrigin::IdentifierNotACall => "unrecovered: identifier not followed by `(`",
        }
    }
}

/// The structure-def name at `span`'s start, when `span` anchors a ctor call.
///
/// Takes the leading Rust-identifier-shaped run at `span.start` and returns it
/// ONLY when the next non-whitespace byte is `(`. That is exactly α's
/// expression-path anchor shape — `compile_builder/entities_phase.rs` sets the
/// label span to `ctor_span.unwrap_or(representative_span)`, the offending
/// `Foo(...)` call's own span — so recovery is exact there.
///
/// Returns `None`, never a guess, for every other shape: the sub `=` path's
/// per-arg anchor (`entity.rs` `PendingBoundCheck`, which starts mid-argument),
/// a plain identifier reference, an out-of-range or prelude-sentinel span, and
/// a `representative_span` fallback of `SourceSpan::empty(0)`. A `None` renders
/// as `—` in the artifact; the def is then named in prose by the two ε codes or
/// left unattributed, which is the honest outcome.
fn ctor_type_name_at(source: &str, span: reify_core::SourceSpan) -> Option<String> {
    ctor_type_name_at_with_origin(source, span).0
}

/// [`ctor_type_name_at`], plus the machine-derived [`DefOrigin`] saying how
/// recovery succeeded or why it failed.
fn ctor_type_name_at_with_origin(
    source: &str,
    span: reify_core::SourceSpan,
) -> (Option<String>, DefOrigin) {
    let start = span.start as usize;
    if start >= source.len() {
        return (None, DefOrigin::SpanOutOfRange);
    }
    // A span that starts inside a multi-byte codepoint cannot be a ctor anchor
    // (identifiers are ASCII-led), and slicing at it would panic.
    if !source.is_char_boundary(start) {
        return (None, DefOrigin::SpanMidCodepoint);
    }
    let rest = &source[start..];
    let mut chars = rest.char_indices();
    // Rust-identifier shape: first char alphabetic or `_`, then alphanumeric
    // or `_`. Reify def names are a subset of this.
    let Some((_, first)) = chars.next() else {
        return (None, DefOrigin::SpanOutOfRange);
    };
    if !(first.is_alphabetic() || first == '_') {
        return (None, DefOrigin::SpanNotIdentifier);
    }
    let ident_end = chars
        .find(|(_, c)| !(c.is_alphanumeric() || *c == '_'))
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    let ident = &rest[..ident_end];
    // Whitespace between the identifier and `(` is legal and must not defeat
    // recovery; anything else means this is not a call.
    let after = rest[ident_end..].trim_start();
    if after.starts_with('(') {
        (Some(ident.to_owned()), DefOrigin::CallSiteAnchor)
    } else {
        (None, DefOrigin::IdentifierNotACall)
    }
}

#[test]
fn line_of_span_is_one_based_and_multibyte_correct() {
    use reify_core::SourceSpan;

    let source = "alpha\nbeta\ngamma\n";
    assert_eq!(
        line_of_span(source, SourceSpan::empty(0)),
        1,
        "offset 0 must be line 1 (1-based, not 0-based)"
    );
    let third = source.find("gamma").expect("fixture has 'gamma'") as u32;
    assert_eq!(
        line_of_span(source, SourceSpan::new(third, third + 5)),
        3,
        "an offset on the third line must be line 3"
    );

    // A multi-byte prefix must not shift the line: `byte_offset_to_line_col`
    // counts codepoints for COLUMNS but newlines for LINES, so a non-ASCII
    // prefix on line 1 leaves an offset on line 2 reporting 2.
    let wide = "π·m·s^-1\nsecond line\n";
    let second = wide.find("second").expect("fixture has 'second'") as u32;
    assert_eq!(
        line_of_span(wide, SourceSpan::new(second, second + 6)),
        2,
        "a multi-byte prefix must not shift the reported line"
    );
}

#[test]
fn line_of_span_clamps_past_eof_instead_of_panicking() {
    use reify_core::SourceSpan;

    // A synthetic / fallback span must never abort a 660-file sweep. Both the
    // plain past-EOF case and the PRELUDE sentinel are exercised: the sentinel
    // is `SourceSpan::empty(u32::MAX)`, which `byte_offset_to_line_col` maps to
    // (1, 1) but which a naive `offset <= len` debug_assert would trip on.
    let source = "one\ntwo\n";
    assert_eq!(
        line_of_span(source, SourceSpan::empty(9_999)),
        2,
        "an offset past EOF must clamp to the last line, not panic"
    );
    assert_eq!(
        line_of_span("", SourceSpan::empty(0)),
        1,
        "an empty source must still report line 1"
    );
    let prelude = line_of_span(source, SourceSpan::prelude());
    assert_eq!(
        prelude, 1,
        "the prelude sentinel must degrade to line 1, not panic or report a wild line"
    );
}

#[test]
fn ctor_type_name_at_recovers_the_def_from_the_call_site_anchor() {
    use reify_core::SourceSpan;

    // α anchors the expression-path label at the ctor call-site's OWN span
    // (compile_builder/entities_phase.rs: `ctor_span.unwrap_or(representative_span)`),
    // so `source[span.start..]` begins with `Widget(` and the leading
    // identifier IS the def name.
    let source = "structure def Root {\n    let x = Widget(label: 42)\n}\n";
    let at_ctor = source.find("Widget(").expect("fixture has 'Widget('") as u32;
    assert_eq!(
        ctor_type_name_at(source, SourceSpan::new(at_ctor, at_ctor + 6)),
        Some("Widget".to_owned()),
        "a span starting at the ctor identifier must recover the def name"
    );
}

#[test]
fn ctor_type_name_at_returns_none_rather_than_guessing() {
    use reify_core::SourceSpan;

    let source = "structure def Root {\n    let x = Widget(label: 42)\n}\n";

    // The sub `=` path anchors PER-ARG (entity.rs `PendingBoundCheck`), so the
    // span starts mid-argument. Recovery must yield None — recorded as `—` in
    // the artifact — never a guessed def name.
    let at_arg = source.find("42").expect("fixture has '42'") as u32;
    assert_eq!(
        ctor_type_name_at(source, SourceSpan::new(at_arg, at_arg + 2)),
        None,
        "a span starting mid-argument must not be mistaken for a ctor anchor"
    );

    // An identifier not followed by `(` is a plain reference, not a ctor.
    let plain = "let y = someBinding + 1\n";
    let at_ident = plain.find("someBinding").expect("fixture has ident") as u32;
    assert_eq!(
        ctor_type_name_at(plain, SourceSpan::new(at_ident, at_ident + 11)),
        None,
        "an identifier not followed by '(' is not a ctor call"
    );

    // Whitespace between the identifier and `(` is still a call.
    let spaced = "let z = Gadget (a: 1)\n";
    let at_g = spaced.find("Gadget").expect("fixture has 'Gadget'") as u32;
    assert_eq!(
        ctor_type_name_at(spaced, SourceSpan::new(at_g, at_g + 6)),
        Some("Gadget".to_owned()),
        "whitespace before '(' must not defeat recovery"
    );

    // Out-of-range and empty-source spans must degrade to None, not panic.
    assert_eq!(ctor_type_name_at(source, SourceSpan::empty(9_999)), None);
    assert_eq!(ctor_type_name_at("", SourceSpan::empty(0)), None);
    assert_eq!(ctor_type_name_at(source, SourceSpan::prelude()), None);
}

// ─── step 5/6: diagnostic field extraction ───────────────────────────────────

/// The diagnostic codes emitted by the struct-ctor field-conformance surface
/// (tasks 5302 / 5303 / 4584 / 4598 / 4622 / 4444) — the admission set shared by
/// this survey and the α corpus gate in the sibling `examples_smoke.rs`.
///
/// This is the SINGLE definition for the whole `harness_compilation_surface`
/// compile unit. `examples_smoke.rs` used to carry its own hand-written copy of
/// the same seven variants; the two were lock-step by convention only, so adding
/// an eighth code to one and not the other would have silently under-counted
/// this survey (or under-gated the α corpus walk). Both now read this slice, so
/// that drift is impossible by construction rather than guarded after the fact.
///
/// # Two copies is NOT the floor — it is where this task's lock set stopped
///
/// A *third* copy lives in
/// `crates/reify-compiler/tests/struct_ctor_field_conformance_tests.rs`
/// (its local `is_ctor_conformance_code`), which is a separate test binary and
/// so cannot reach this `#[path]` module. That copy is still lock-step by
/// convention, with no drift guard — the exact failure mode collapsing the
/// first two removed.
///
/// The support-crate hop that would close it ALREADY EXISTS and costs nothing
/// new: `struct_ctor_field_conformance_tests.rs` already does
/// `use reify_test_support::{…}`, this module already does
/// `use reify_test_support::git_env::{…}`, and `reify-test-support` already
/// carries `reify-core.workspace = true`, so `DiagnosticCode` is in scope
/// there. The right home is a `ctor_conformance` module in
/// `reify-test-support` alongside `git_env`, read by all three consumers.
///
/// It is not done here because landing it means editing
/// `crates/reify-test-support/src/lib.rs` and
/// `struct_ctor_field_conformance_tests.rs`, neither of which is in task
/// #5304's lock set — a concurrency-footprint expansion, not a technical
/// obstacle. Filed as follow-up rather than asserted away: do NOT read the
/// paragraph above as a rationale for why two copies are acceptable.
pub(super) const CTOR_CONFORMANCE_CODES: &[reify_core::diagnostics::DiagnosticCode] = {
    use reify_core::diagnostics::DiagnosticCode;
    &[
        DiagnosticCode::ArgTypeMismatch,
        DiagnosticCode::SelectorKindMismatch,
        DiagnosticCode::TypeNotConformingToTrait,
        DiagnosticCode::TypeNotConformingToStructureRef,
        DiagnosticCode::TypeNotConformingToVector,
        DiagnosticCode::CtorUnknownField,
        DiagnosticCode::CtorArity,
    ]
};

/// True when `code` is one of [`CTOR_CONFORMANCE_CODES`].
///
/// Shared with the α corpus gate in `examples_smoke.rs`, which calls straight
/// through to it — see [`CTOR_CONFORMANCE_CODES`] for why there is exactly one
/// definition in this compile unit.
pub(super) fn is_ctor_conformance_code(code: Option<reify_core::diagnostics::DiagnosticCode>) -> bool {
    code.is_some_and(|c| CTOR_CONFORMANCE_CODES.contains(&c))
}

/// Which D9 fix-forward rule governs a site — the load-bearing, mechanizable
/// half of D9 and the artifact's primary grouping key.
///
/// This does NOT encode D9's split between class (1) call-site bug and class
/// (2) wrong declared field type. The PRD defines that as "per-case judgment …
/// whichever is the actual bug" and assigns it to γ; β must not fabricate it.
///
/// # The derives are load-bearing, not decoration
///
/// `EnumIter` + `Ord` are what make [`Owner::render_order`] DERIVED from this
/// declaration instead of restated as an array literal at the render site. The
/// variant order below therefore IS the artifact's group order, and adding a
/// variant automatically adds its group. See [`Owner::render_order`] for why
/// that matters more here than anywhere else in the module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum::EnumIter)]
enum Owner {
    /// The site's structure def is declared in an FEA stdlib module. Per D9,
    /// γ may make CALL-SITE changes only — field-type flips stay v0.6-owned.
    FeaDeferredToV06,
    /// The recovered name IS a declared `structure def` somewhere in the swept
    /// corpus (or the stdlib) and is not FEA-owned: D9's per-case judgment
    /// applies, and this is the only bucket γ should size as actionable.
    NonFea,
    /// A name was recovered at the anchor, but it is not a declared
    /// `structure def` anywhere.
    ///
    /// [`ctor_type_name_at`] recovers *any* identifier followed by `(` — it
    /// cannot tell a ctor from a plain function call. Several codes in the
    /// admission set (`SelectorKindMismatch` from selector composition and
    /// overload resolution) reach this survey from a NON-ctor path, where the
    /// anchor identifier is a FUNCTION name: `union(faces(b), edges(b))` and
    /// `needs_face(edges(b))` both appear in the tracked corpus. Letting those
    /// fall into [`Owner::NonFea`] would size a function call into γ's
    /// actionable pile — exactly the "unattributable site must never default
    /// into the touchable bucket" rule this module states, violated one level
    /// down from where it was being enforced.
    UnresolvedDef,
    /// No def name could be attributed at all — the label span names none and
    /// the diagnostic prose names none. Each row carries its own machine-derived
    /// [`DefOrigin`] saying which shape it was.
    ///
    /// Deliberately its own bucket: silently defaulting an unattributable site
    /// into the touchable pile would be the one classification error with a real
    /// cost.
    Unknown,
}

impl Owner {
    /// Every owner class, in the order the artifact renders their groups.
    ///
    /// DERIVED from the enum declaration via `strum::EnumIter` and ordered by
    /// the `Ord` derive — deliberately NOT an array literal at the render site.
    /// A hand-written group list is the one drift this artifact cannot afford:
    /// a fifth `Owner` variant would compile cleanly, render no group at all,
    /// and silently drop every site classified into it — while the header still
    /// printed `**Sites:** N` counting it. "A class the renderer forgets is a
    /// class of sites that silently vanishes from the artifact" is this
    /// module's own statement of its one unacceptable failure; a literal makes
    /// that failure reachable by omission, and a guard test that iterates its
    /// OWN copy of the same literal cannot see it either.
    ///
    /// `strum` is already a `[dev-dependencies]` entry of this crate (the ε2
    /// `TypeDiscriminants` canary), so this costs no new dependency.
    fn render_order() -> Vec<Owner> {
        use strum::IntoEnumIterator;
        let mut all: Vec<Owner> = Owner::iter().collect();
        // The FEA do-not-touch partition first, then the actionable non-FEA
        // group, then the two manual-triage buckets — which is exactly the
        // declaration order, pinned here through `Ord` rather than assumed
        // from `EnumIter`'s traversal.
        all.sort_unstable();
        all
    }

    /// Stable section title for the rendered artifact.
    fn title(self) -> &'static str {
        match self {
            Owner::FeaDeferredToV06 => "FEA — deferred to v0.6 (DO NOT FIX HERE)",
            Owner::NonFea => "non-FEA structure def — γ per-case judgment",
            Owner::UnresolvedDef => {
                "name recovered, but it is not a known structure def — needs manual triage"
            }
            Owner::Unknown => "unattributed def — needs manual triage",
        }
    }
}

/// One ctor-conformance warning site, as one row of the survey artifact.
///
/// Every field is machine-derived; nothing here is ever typed in by hand. An
/// extractor that cannot recover its column yields `None`, which renders as an
/// em-dash — never an empty cell and never a guess.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SurveySite {
    /// Repo-relative forward-slash path of the swept file.
    file: String,
    /// 1-based line of the diagnostic's first label span (or 1 when unlabelled).
    line: u32,
    /// Structure def being constructed, when recoverable.
    def: Option<String>,
    /// How [`SurveySite::def`] was recovered — or, when it is `None`, why it
    /// could not be. Machine-derived; see [`DefOrigin`].
    def_origin: DefOrigin,
    /// Offending field / param name, when the wording carries one.
    field: Option<String>,
    /// Declared param type, from the `expected '<X>', got '<Y>'` label.
    expected: Option<String>,
    /// Supplied arg type, from the same label.
    found: Option<String>,
    /// `Debug` rendering of the `DiagnosticCode` (PascalCase).
    code: String,
    /// `Debug` rendering of the measured `Severity` — reported, not assumed.
    severity: String,
    /// The diagnostic's raw message, preserved verbatim.
    message: String,
    /// D9 owner class. Assigned by the corpus sweep via [`d9_owner`]; the
    /// builder leaves it `Unknown`, the conservative default.
    owner: Owner,
}

/// The `emit_arg_type_mismatch` prose prefix that introduces the offending param
/// name. Also a substring of `emit_geometry_trait_violation`'s
/// `geometry argument '` and of ε's `unknown named argument '`, so one search
/// covers six of the seven codes.
const ARG_PREFIX: &str = "argument '";

/// The prefix used by the two non-geometry `TypeNotConformingToTrait` emitters
/// (`type 'X' does not conform to trait 'T' required by param 'f'`), which name
/// the param nowhere else.
const REQUIRED_BY_PARAM_PREFIX: &str = "required by param '";

/// The ε `CtorUnknownField` prose that names the target structure def.
const IN_CALL_TO_PREFIX: &str = "in call to '";

/// The ε `CtorArity` message prefix; the def name follows it, up to `()`.
const CTOR_ARITY_PREFIX: &str = "E_CTOR_ARITY: ";

/// The single-quoted token immediately following `prefix` in `haystack`.
///
/// This is the guarded quoted-token idiom from `examples_smoke.rs`'s
/// `param_name_from_ctor_diagnostic`, lifted verbatim rather than re-invented,
/// and it carries that helper's warning forward: **this is a real coupling to
/// diagnostic prose.** Every extractor built on it therefore returns `Option`,
/// and [`survey_site_from_diagnostic`] preserves the RAW message on a miss, so
/// a future wording drift degrades to a still-usable row instead of a silently
/// dropped site or a fabricated field.
///
/// An EMPTY token (`prefix` immediately followed by the closing quote) is a
/// miss, not a hit: `Some("")` names nothing, and every caller here would have
/// to re-filter it. Rejecting it at the source keeps that rule in ONE place —
/// and, more importantly, keeps the `or_else` fallback chains below live. An
/// earlier draft filtered AFTER the chain
/// (`quoted_after(A).or_else(|| quoted_after(B)).filter(non-empty)`), where a
/// `Some("")` from `A` short-circuits `or_else` and only then filters to
/// `None` — so `B` is never consulted and the "fallback" is conditionally dead.
fn quoted_after(haystack: &str, prefix: &str) -> Option<String> {
    let start = haystack.find(prefix)? + prefix.len();
    let rest = &haystack[start..];
    let end = rest.find('\'')?;
    let token = &rest[..end];
    (!token.is_empty()).then(|| token.to_owned())
}

/// The offending field / param name, across every wording the 7 codes use.
///
/// Returns `None` for `CtorArity` (whose wording names no param) and for any
/// message that has drifted out of all three known shapes.
fn field_of_message(message: &str) -> Option<String> {
    quoted_after(message, ARG_PREFIX).or_else(|| quoted_after(message, REQUIRED_BY_PARAM_PREFIX))
}

/// `(expected, found)` from a `expected '<X>', got '<Y>'` LABEL.
///
/// The label is preferred over the prose main message because it is exactly
/// that shape (`conformance/mod.rs` `emit_arg_type_mismatch` and its three
/// siblings), whereas the message interleaves the param name twice and may
/// carry the D4-6 dimensioned-scalar migration hint after a `;`. The two ε
/// codes carry no such label and correctly yield `(None, None)`.
fn expected_found_of_labels(d: &reify_core::Diagnostic) -> (Option<String>, Option<String>) {
    for label in &d.labels {
        let (Some(expected), Some(found)) = (
            quoted_after(&label.message, "expected '"),
            quoted_after(&label.message, "got '"),
        ) else {
            continue;
        };
        return (Some(expected), Some(found));
    }
    (None, None)
}

/// The structure def being constructed, when recoverable.
///
/// Three sources, in order of reliability:
/// 1. The ε `CtorUnknownField` prose `in call to '<Def>'`.
/// 2. The ε `CtorArity` prose `<Def>() expects at most …`.
/// 3. The call-site span anchor — α anchors the expression-path label at the
///    ctor's own span, so `source[span.start..]` begins with `Def(`.
///
/// Each prose shape is tried ONLY for the code that emits it. An earlier draft
/// ran both prefix matches against every admitted code, which is broader than
/// the contract above and opens the one hole this module cannot afford: any
/// future `ArgTypeMismatch` / `TypeNotConformingToTrait` wording that happened
/// to contain `in call to '<X>'` would be attributed to `<X>` as
/// [`DefOrigin::DiagnosticProse`], bypassing the call-site anchor and its
/// `IdentifierNotACall` / `SpanNotIdentifier` diagnosis — a def GUESSED from a
/// sentence rather than read off an anchor. Gating on `d.code` costs nothing
/// (the ε emitters are the only source of either prefix) and closes it.
///
/// Returns `None` — never a guess — for every anchor shape that names no def,
/// PAIRED WITH the machine-derived [`DefOrigin`] saying which shape it was, so
/// the artifact can report the cause instead of asserting one.
fn def_of_diagnostic(source: &str, d: &reify_core::Diagnostic) -> (Option<String>, DefOrigin) {
    use reify_core::diagnostics::DiagnosticCode;

    if d.code == Some(DiagnosticCode::CtorUnknownField)
        && let Some(def) = quoted_after(&d.message, IN_CALL_TO_PREFIX)
    {
        return (Some(def), DefOrigin::DiagnosticProse);
    }
    if d.code == Some(DiagnosticCode::CtorArity)
        && let Some(rest) = d.message.strip_prefix(CTOR_ARITY_PREFIX)
        && let Some(paren) = rest.find("()")
        && !rest[..paren].is_empty()
    {
        return (Some(rest[..paren].to_owned()), DefOrigin::DiagnosticProse);
    }
    match d.labels.first() {
        None => (None, DefOrigin::NoLabel),
        Some(l) => ctor_type_name_at_with_origin(source, l.span),
    }
}

/// Build one [`SurveySite`] from a diagnostic observed while sweeping `file`.
///
/// Returns `None` only when `d` is not one of the 7 ctor-conformance codes (an
/// uncoded legacy diagnostic included). A ctor-coded diagnostic ALWAYS yields a
/// row, even when every extractor misses — dropping it would silently
/// under-size γ, which is the artifact's whole purpose.
///
/// `owner` is left [`Owner::Unknown`]; the corpus sweep assigns it via
/// [`d9_owner`] once the FEA def set has been scanned.
fn survey_site_from_diagnostic(
    file: &str,
    source: &str,
    d: &reify_core::Diagnostic,
) -> Option<SurveySite> {
    if !is_ctor_conformance_code(d.code) {
        return None;
    }
    let (expected, found) = expected_found_of_labels(d);
    let line = d
        .labels
        .first()
        .map(|l| line_of_span(source, l.span))
        .unwrap_or(1);
    let (def, def_origin) = def_of_diagnostic(source, d);
    Some(SurveySite {
        file: file.to_owned(),
        line,
        def,
        def_origin,
        field: field_of_message(&d.message),
        expected,
        found,
        code: format!("{:?}", d.code.expect("filtered to Some(code) above")),
        severity: format!("{:?}", d.severity),
        message: d.message.clone(),
        owner: Owner::Unknown,
    })
}

/// Build a synthetic diagnostic in the exact shape a given emitter produces, so
/// the extractor tests need no compilation at all.
#[cfg(test)]
fn synth(
    code: reify_core::diagnostics::DiagnosticCode,
    message: &str,
    label: Option<&str>,
) -> reify_core::Diagnostic {
    use reify_core::{Severity, diagnostics::DiagnosticLabel};
    let mut d = reify_core::Diagnostic::error(message).with_code(code);
    // α's knob: ctor field conformance is Warning severity, not Error.
    d.severity = Severity::Warning;
    if let Some(l) = label {
        d = d.with_label(DiagnosticLabel::new(reify_core::SourceSpan::empty(0), l));
    }
    d
}

#[test]
fn survey_site_extracts_field_from_the_argument_prose_prefix() {
    use reify_core::diagnostics::DiagnosticCode;

    // `emit_arg_type_mismatch` (conformance/mod.rs) — the dominant shape.
    let d = synth(
        DiagnosticCode::ArgTypeMismatch,
        "argument 'label' has type 'Int' but param 'label' requires type 'String'",
        Some("expected 'String', got 'Int'"),
    );
    let site = survey_site_from_diagnostic("a.ri", "", &d).expect("ctor-coded diag yields a site");
    assert_eq!(site.field.as_deref(), Some("label"));

    // `emit_selector_mismatch` kind-vs-kind — same `argument '` prefix. The
    // kind renderings are CONSTRUCTED from `reify_core::Type` rather than
    // transcribed: `emit_selector_mismatch` (conformance/mod.rs) interpolates
    // the `Type` Display, which is `FaceSelector`/`EdgeSelector`. An earlier
    // draft of this fixture wrote `Selector(Face)` — a string the compiler never
    // emits, and exactly the bug `is_selector_type` already shipped once — which
    // this helper's "the exact shape a given emitter produces" contract forbids.
    let face = reify_core::Type::Selector(reify_core::ty::SelectorKind::Face).to_string();
    let edge = reify_core::Type::Selector(reify_core::ty::SelectorKind::Edge).to_string();
    let d = synth(
        DiagnosticCode::SelectorKindMismatch,
        &format!(
            "argument 'face' has selector kind '{edge}' but param 'face' \
             requires selector kind '{face}'"
        ),
        Some(&format!("expected '{face}', got '{edge}'")),
    );
    let site = survey_site_from_diagnostic("a.ri", "", &d).expect("site");
    assert_eq!(site.field.as_deref(), Some("face"));

    // `emit_geometry_trait_violation` — the prefix is `geometry argument '`,
    // which still CONTAINS `argument '`, so the same extractor recovers it.
    let d = synth(
        DiagnosticCode::TypeNotConformingToTrait,
        "geometry argument 'target' does not conform to trait 'Solid'",
        Some("geometry argument 'target' is not Solid"),
    );
    let site = survey_site_from_diagnostic("a.ri", "", &d).expect("site");
    assert_eq!(site.field.as_deref(), Some("target"));

    // The OTHER `TypeNotConformingToTrait` shape names the param via a
    // different prefix entirely: `required by param '<name>'`.
    let d = synth(
        DiagnosticCode::TypeNotConformingToTrait,
        "type 'Bolt' does not conform to trait 'Fastener' required by param 'part'",
        Some("type 'Bolt' does not conform to trait 'Fastener'"),
    );
    let site = survey_site_from_diagnostic("a.ri", "", &d).expect("site");
    assert_eq!(
        site.field.as_deref(),
        Some("part"),
        "the `required by param '` shape must also yield a field"
    );

    // `emit_structure_ref_mismatch` / `emit_vector_mismatch`.
    for (code, msg) in [
        (
            DiagnosticCode::TypeNotConformingToStructureRef,
            "argument 'part' has type 'Int' but param 'part' requires structure type 'Part'",
        ),
        (
            DiagnosticCode::TypeNotConformingToVector,
            "argument 'axis' has type 'Real' but param 'axis' requires vector type 'Vector3<Length>'",
        ),
    ] {
        let d = synth(code, msg, Some("expected 'X', got 'Y'"));
        let site = survey_site_from_diagnostic("a.ri", "", &d).expect("site");
        assert!(
            site.field.is_some(),
            "{code:?} must yield a field, got None"
        );
    }
}

#[test]
fn an_empty_quoted_token_is_a_miss_so_the_fallback_prefix_is_still_consulted() {
    // `quoted_after` rejects an empty token at the SOURCE rather than leaving
    // each caller to re-filter, so `field_of_message`'s `or_else` chain stays a
    // real fallback. Filtering after the chain instead makes the second prefix
    // conditionally dead: `Some("")` from the first satisfies `or_else`, the
    // filter then turns it into `None`, and a recoverable field renders `—`.
    assert_eq!(
        quoted_after("argument '' has type", ARG_PREFIX),
        None,
        "an empty quoted token names nothing and must not be reported as a hit"
    );
    assert_eq!(
        field_of_message("argument '' … required by param 'part'"),
        Some("part".to_owned()),
        "an empty first token must fall THROUGH to `required by param '`, not \
         short-circuit the chain into None"
    );

    // Positive control, so the assertion above cannot pass vacuously, plus the
    // genuine no-match case.
    assert_eq!(
        field_of_message("argument 'label' has type 'Int'"),
        Some("label".to_owned())
    );
    assert_eq!(
        field_of_message("E_CTOR_ARITY: W() expects at most 1 argument, got 3"),
        None
    );

    // `def_of_diagnostic`'s ε prose path has the same latent shape and is
    // covered by the same source-level rule.
    assert_eq!(quoted_after("in call to ''; …", IN_CALL_TO_PREFIX), None);
}

#[test]
fn survey_site_extracts_field_and_def_from_the_epsilon_codes() {
    use reify_core::diagnostics::DiagnosticCode;

    // ε `CtorUnknownField` (expr.rs): names BOTH the field and the def.
    let d = synth(
        DiagnosticCode::CtorUnknownField,
        "E_CTOR_UNKNOWN_FIELD: unknown named argument 'wgt' in call to 'Bar'; \
         'Bar' has no parameter with that name",
        Some("unknown named argument"),
    );
    let site = survey_site_from_diagnostic("a.ri", "", &d).expect("site");
    assert_eq!(site.field.as_deref(), Some("wgt"));
    assert_eq!(
        site.def.as_deref(),
        Some("Bar"),
        "the `in call to '<Def>'` prose must supply the def"
    );

    // ε `CtorArity` names the def but NO param — its wording carries none.
    let d = synth(
        DiagnosticCode::CtorArity,
        "E_CTOR_ARITY: Bar() expects at most 2 arguments, got 3",
        Some("wrong number of arguments"),
    );
    let site = survey_site_from_diagnostic("a.ri", "", &d).expect("site");
    assert_eq!(
        site.field, None,
        "CtorArity names no param — inventing one would be a fabricated row"
    );
    assert_eq!(site.def.as_deref(), Some("Bar"));
}

#[test]
fn epsilon_prose_is_only_consulted_for_the_epsilon_codes() {
    use reify_core::diagnostics::DiagnosticCode;

    // A NON-ε code whose message happens to contain the ε prose. Nothing stops
    // a future `emit_*` wording from reading "… in call to 'Bar'": the phrase is
    // ordinary English, not a reserved token. If the prose match were tried for
    // every admitted code, `Bar` would be lifted out of that sentence and
    // recorded as the def — a name GUESSED from prose, with `DiagnosticProse`
    // vouching for it, and the call-site anchor never consulted.
    let source = "let x = 42\n";
    let mut d = synth(
        DiagnosticCode::ArgTypeMismatch,
        "argument 'label' has type 'Int' but param 'label' requires type 'String' \
         in call to 'Bar'",
        None,
    );
    d = d.with_label(reify_core::diagnostics::DiagnosticLabel::new(
        // Anchored at the `42`, i.e. a shape that names no def.
        reify_core::SourceSpan::new(8, 10),
        "expected 'String', got 'Int'",
    ));
    let site = survey_site_from_diagnostic("a.ri", source, &d).expect("site");
    assert_eq!(
        site.def, None,
        "`in call to '<X>'` must be read ONLY for CtorUnknownField; for any other \
         code the def comes from the anchor or not at all"
    );
    assert_eq!(
        site.def_origin,
        DefOrigin::SpanNotIdentifier,
        "the row must carry the anchor's own machine-derived cause, not `DiagnosticProse`"
    );

    // Same shape for the arity prefix: a non-`CtorArity` code that literally
    // starts with it still routes to the anchor.
    let d = synth(
        DiagnosticCode::CtorUnknownField,
        "E_CTOR_ARITY: Bar() expects at most 2 arguments, got 3",
        None,
    );
    let site = survey_site_from_diagnostic("a.ri", source, &d).expect("site");
    assert_eq!(
        site.def, None,
        "the `E_CTOR_ARITY: ` prefix must be read ONLY for CtorArity"
    );
    assert_eq!(site.def_origin, DefOrigin::NoLabel);
}

#[test]
fn survey_site_prefers_the_label_for_expected_and_found() {
    use reify_core::diagnostics::DiagnosticCode;

    // The LABEL is more structured than the prose main message: it is exactly
    // `expected '<X>', got '<Y>'` (conformance/mod.rs emit_arg_type_mismatch),
    // whereas the message interleaves the param name twice and may carry the
    // D4-6 dimensioned-scalar migration hint after a `;`.
    let d = synth(
        DiagnosticCode::ArgTypeMismatch,
        "argument 'velocity_limit' has type 'Real' but param 'velocity_limit' requires \
         type 'Scalar[m·s^-1]'; pass a dimensioned literal such as 1m/s",
        Some("expected 'Scalar[m·s^-1]', got 'Real'"),
    );
    let site = survey_site_from_diagnostic("a.ri", "", &d).expect("site");
    assert_eq!(site.expected.as_deref(), Some("Scalar[m·s^-1]"));
    assert_eq!(site.found.as_deref(), Some("Real"));
    assert_eq!(site.field.as_deref(), Some("velocity_limit"));

    // The two ε codes carry no `expected '…', got '…'` label at all.
    for (code, msg, label) in [
        (
            DiagnosticCode::CtorUnknownField,
            "E_CTOR_UNKNOWN_FIELD: unknown named argument 'w' in call to 'Bar'; \
             'Bar' has no parameter with that name",
            "unknown named argument",
        ),
        (
            DiagnosticCode::CtorArity,
            "E_CTOR_ARITY: Bar() expects at most 1 argument, got 2",
            "wrong number of arguments",
        ),
    ] {
        let d = synth(code, msg, Some(label));
        let site = survey_site_from_diagnostic("a.ri", "", &d).expect("site");
        assert_eq!(
            site.expected, None,
            "{code:?} carries no expected/got label"
        );
        assert_eq!(site.found, None, "{code:?} carries no expected/got label");
    }
}

#[test]
fn survey_site_renders_code_and_severity_in_pascal_case_debug_form() {
    use reify_core::diagnostics::DiagnosticCode;

    let d = synth(
        DiagnosticCode::ArgTypeMismatch,
        "argument 'a' has type 'Int' but param 'a' requires type 'String'",
        Some("expected 'String', got 'Int'"),
    );
    let site = survey_site_from_diagnostic("a.ri", "", &d).expect("site");
    // `{:?}` is used because reify-core's serde feature (which supplies the
    // PascalCase wire name) is non-default and not enabled for reify-compiler.
    // Debug renders the identical string — same choice as examples_smoke.rs.
    assert_eq!(site.code, "ArgTypeMismatch");
    assert_eq!(
        site.severity, "Warning",
        "α's knob is Warning; the sweep must report what it measured, not assume Error"
    );
}

#[test]
fn survey_site_degrades_to_none_fields_and_keeps_the_raw_message() {
    use reify_core::diagnostics::DiagnosticCode;

    // Extraction is a REAL coupling to diagnostic prose (the warning carried
    // forward from examples_smoke.rs's param_name_from_ctor_diagnostic). A
    // future wording drift must degrade to a still-usable row — never a
    // silently dropped site, and never a fabricated field.
    let drifted = "the wording of this diagnostic drifted entirely";
    let d = synth(DiagnosticCode::ArgTypeMismatch, drifted, None);
    let site = survey_site_from_diagnostic("a.ri", "", &d)
        .expect("an unrecognised message must still yield a row — dropping it would under-size γ");
    assert_eq!(site.field, None);
    assert_eq!(site.expected, None);
    assert_eq!(site.found, None);
    assert_eq!(site.def, None);
    assert_eq!(
        site.message, drifted,
        "the RAW message must survive verbatim so a drifted row stays usable"
    );
}

#[test]
fn survey_site_rejects_non_ctor_conformance_diagnostics() {
    use reify_core::diagnostics::DiagnosticCode;

    // A code outside the 7-variant set is not a survey site.
    let d = synth(
        DiagnosticCode::TraitNotImplemented,
        "type 'Bolt' does not implement trait 'Fastener'",
        None,
    );
    assert!(
        survey_site_from_diagnostic("a.ri", "", &d).is_none(),
        "only the 7 ctor-conformance codes may enter the survey"
    );

    // An uncoded (legacy) diagnostic is likewise not a site.
    let mut uncoded = reify_core::Diagnostic::error("argument 'a' has type 'Int'");
    uncoded.severity = reify_core::Severity::Warning;
    assert!(survey_site_from_diagnostic("a.ri", "", &uncoded).is_none());
}

#[test]
fn survey_site_carries_file_and_resolved_line() {
    use reify_core::{Severity, diagnostics::DiagnosticCode, diagnostics::DiagnosticLabel};

    let source = "module test.x\nstructure def Root {\n    let q = Widget(label: 42)\n}\n";
    let at_ctor = source.find("Widget(").expect("fixture") as u32;
    let mut d = reify_core::Diagnostic::error(
        "argument 'label' has type 'Int' but param 'label' requires type 'String'",
    )
    .with_code(DiagnosticCode::ArgTypeMismatch)
    .with_label(DiagnosticLabel::new(
        reify_core::SourceSpan::new(at_ctor, at_ctor + 6),
        "expected 'String', got 'Int'",
    ));
    d.severity = Severity::Warning;

    let site = survey_site_from_diagnostic("examples/x.ri", source, &d).expect("site");
    assert_eq!(site.file, "examples/x.ri");
    assert_eq!(site.line, 3, "the ctor is on line 3 of the fixture");
    assert_eq!(
        site.def.as_deref(),
        Some("Widget"),
        "the call-site anchor must recover the def name for the expression path"
    );
    // Not yet classified: `d9_owner` is applied by the corpus sweep, and the
    // conservative default is the unattributable bucket, never `NonFea`.
    assert_eq!(site.owner, Owner::Unknown);
}

// ─── step 7/8: D9 owner classification ───────────────────────────────────────

/// Absolute path to the stdlib directory whose FEA modules define the
/// do-not-touch partition.
const STDLIB_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/stdlib");

/// The FEA stdlib module FILE STEMS, as PRD §4 D9 itself enumerates them
/// ("`fea_multi_case.ri`, `fea.ri`, `solver_*.ri`, …"), reconciled against the
/// live `ls crates/reify-compiler/stdlib/`.
///
/// This small MODULE list is the survey's only reviewable knob. The def NAMES
/// are always derived by scanning these files, never hand-listed — which is
/// what keeps "zero hand-derived entries" literally true, and keeps the
/// do-not-touch partition traceable to the PRD rather than to β's judgment.
///
/// [`scan_structure_defs`] panics if any stem here has no `.ri` file, so a
/// stdlib rename cannot silently empty the partition.
const FEA_STDLIB_MODULES: &[&str] = &[
    "fea",
    "fea_multi_case",
    "fea_types",
    "materials_fea",
    "solver_buckling",
    "solver_buckling_fns",
    "solver_elastic",
];

/// Stdlib modules whose NAME reads as FEA-family but which are deliberately NOT
/// in the D9 do-not-touch partition.
///
/// This list exists so that "not FEA-owned" is a RECORDED decision rather than
/// an omission. [`every_fea_family_shaped_stdlib_module_is_classified`] requires
/// every FEA-shaped stem to appear in exactly one of the two lists, so adding
/// `fea_contact.ri` (or a fourth `modal_*`) to the stdlib turns that guard red
/// instead of silently routing its defs into `Owner::NonFea` — the group the
/// artifact labels "the group to size γ against". Mis-classifying INTO that
/// group is, per [`d9_owner`], "the one classification error with a real cost",
/// and until this guard existed it was the only classification path with no
/// drift check at all.
///
/// Why `modal_*` is on THIS side of the line: PRD §4 D9 defines the deferred
/// partition by the v0.6 migration it points at
/// (`docs/prds/v0_6/fea-load-support-selector-migration.md`) — the FEA load and
/// boundary-condition defs whose String→selector field flips are v0.6-owned —
/// and enumerates it as "`fea_multi_case.ri`, `fea.ri`, `solver_*.ri`, …".
/// `modal_analysis.ri` is structural dynamics, not that migration's surface: its
/// forcing-function defs already declare selector-typed fields
/// (`structure def StepForce { param at : Selector … }`, `modal_analysis.ri:490`),
/// so a ctor row against one of them is ordinary call-site work for γ, with no
/// field-type flip to defer. The two `modal_*_fns` modules declare no
/// `structure def` at all, so their placement is inert either way and is
/// recorded only to keep the shape sweep exhaustive.
const DELIBERATELY_NOT_FEA_OWNED: &[&str] = &[
    "modal_analysis",
    "modal_analysis_fns",
    "modal_mechanism_fns",
];

/// True for a stdlib module stem that reads as FEA-family.
///
/// Deliberately WIDER than [`FEA_STDLIB_MODULES`]: its job is to catch a new
/// module that a reader would plausibly expect in the deferred partition, and
/// force a classification. A name outside every shape here (say a future
/// `contact_mechanics.ri`) is not caught — no naming rule can be complete —
/// which is why the FEA list stays a reviewable knob rather than a derived one.
fn is_fea_family_shaped(stem: &str) -> bool {
    stem == "fea"
        || stem.starts_with("fea_")
        || stem.ends_with("_fea")
        || stem.starts_with("solver_")
        || stem.starts_with("modal_")
}

#[test]
fn every_fea_family_shaped_stdlib_module_is_classified() {
    let dir = std::path::Path::new(STDLIB_DIR);
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read the stdlib dir {}: {e}", dir.display()));

    let mut shaped: Vec<String> = Vec::new();
    for entry in entries {
        let path = entry
            .unwrap_or_else(|e| panic!("cannot read an entry of {}: {e}", dir.display()))
            .path();
        if path.extension().and_then(|e| e.to_str()) != Some("ri") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned();
        if is_fea_family_shaped(&stem) {
            shaped.push(stem);
        }
    }
    shaped.sort();
    assert!(
        !shaped.is_empty(),
        "the shape sweep matched NO stdlib module — the enumeration or the shape \
         predicate has broken, and this guard would be vacuously green"
    );

    let unclassified: Vec<&String> = shaped
        .iter()
        .filter(|stem| {
            !FEA_STDLIB_MODULES.contains(&stem.as_str())
                && !DELIBERATELY_NOT_FEA_OWNED.contains(&stem.as_str())
        })
        .collect();
    assert!(
        unclassified.is_empty(),
        "these stdlib modules read as FEA-family but are in neither \
         FEA_STDLIB_MODULES nor DELIBERATELY_NOT_FEA_OWNED: {unclassified:?}. \
         Leaving one off is not inert: its defs route to `Owner::NonFea`, the \
         group the artifact tells γ to size and fix. Add it to whichever list is \
         right — and say why, if it is the second."
    );

    let both: Vec<&&str> = FEA_STDLIB_MODULES
        .iter()
        .filter(|m| DELIBERATELY_NOT_FEA_OWNED.contains(m))
        .collect();
    assert!(
        both.is_empty(),
        "a module cannot be both FEA-owned and deliberately not: {both:?}"
    );

    // Same rename guard `scan_structure_defs` gives the FEA list, for the other
    // one: a stale exclusion is how a genuinely FEA-shaped NEW module can slip
    // past the check above under an old name.
    for stem in DELIBERATELY_NOT_FEA_OWNED {
        let path = dir.join(format!("{stem}.ri"));
        assert!(
            path.exists(),
            "DELIBERATELY_NOT_FEA_OWNED lists '{stem}' but {} does not exist — a \
             stdlib rename must not leave a stale exclusion behind",
            path.display()
        );
    }
}

/// The `structure def <Name>` declarations in `dir/<stem>.ri` for each `stem`.
///
/// Anchored at COLUMN 0 rather than matched as a substring, deliberately: a
/// naive scan of `stdlib/fea_multi_case.ri` harvests `already` as a def name
/// from the prose "…(its structure def already declares…" in a comment at line
/// 292. Every real declaration in the stdlib is at column 0, and a `//` line is
/// skipped outright, so both halves of that guard are cheap.
///
/// # Panics
///
/// If a listed module has no file. That is the deliberate loud failure: a
/// silently-empty FEA partition would mis-classify every v0.6-deferred site as
/// touchable, which is the single most costly error this artifact could make.
fn scan_structure_defs(
    dir: &std::path::Path,
    modules: &[&str],
) -> std::collections::BTreeSet<String> {
    let mut defs = std::collections::BTreeSet::new();
    for stem in modules {
        let path = dir.join(format!("{stem}.ri"));
        let source = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "ctor_conformance_corpus_survey: FEA stdlib module '{stem}' is listed in \
                 FEA_STDLIB_MODULES but {} cannot be read: {e}. A stdlib rename must not \
                 silently empty the D9 do-not-touch partition — update the list.",
                path.display()
            )
        });
        collect_structure_defs_into(&source, &mut defs);
    }
    defs
}

/// Add every `structure def <Name>` declared by `source` to `defs`.
///
/// The column-0 anchor and the `pub `/`priv ` visibility prefixes are the whole
/// grammar: measured over the tracked corpus, all 719 declarations sit at column
/// 0 and 10 of them carry `pub `. Missing the visibility prefix would drop
/// `pub structure def Actuator` from the known set and demote its sites to
/// [`Owner::UnresolvedDef`] — conservative, but needless noise in γ's triage.
fn collect_structure_defs_into(source: &str, defs: &mut std::collections::BTreeSet<String>) {
    const DEF_KEYWORD: &str = "structure def ";
    const VISIBILITY_PREFIXES: &[&str] = &["pub ", "priv "];
    for line in source.lines() {
        // Column-0 anchor: skips comments and any nested/indented prose. A naive
        // substring scan of `stdlib/fea_multi_case.ri` harvests `already` from
        // the comment "…(its structure def already declares…".
        let after_vis = VISIBILITY_PREFIXES
            .iter()
            .find_map(|p| line.strip_prefix(p))
            .unwrap_or(line);
        let Some(rest) = after_vis.strip_prefix(DEF_KEYWORD) else {
            continue;
        };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            defs.insert(name);
        }
    }
}

/// Every `structure def` declared anywhere in `crates/reify-compiler/stdlib/`.
///
/// Seeds the known-def set so a site constructing a stdlib def still resolves
/// even when the declaring stdlib file was not itself part of the swept corpus.
///
/// # Panics
///
/// On ANY I/O failure — an unreadable directory entry, or a `*.ri` under
/// [`STDLIB_DIR`] that cannot be read. Same loud-failure contract as
/// [`scan_structure_defs`], and for the same reason: a silently-shrunk known-def
/// set does not fail visibly, it DEMOTES real ctor sites to
/// [`Owner::UnresolvedDef`] with no signal anywhere in the artifact. An earlier
/// draft swallowed both failures (`entries.flatten()` and an `if let Ok(…)`),
/// which is the same silent-shrink class the sibling scanner already panics on.
///
/// Scanned ONCE per process, behind the same `OnceLock` its FEA sibling
/// [`fea_owned_defs`] uses: the stdlib does not change while the test binary
/// runs, and every gate-resident test that reaches `survey_corpus` would
/// otherwise re-`read_dir` and re-read all ~46 modules from disk.
fn stdlib_structure_defs() -> &'static std::collections::BTreeSet<String> {
    static DEFS: std::sync::OnceLock<std::collections::BTreeSet<String>> =
        std::sync::OnceLock::new();
    DEFS.get_or_init(scan_stdlib_structure_defs)
}

/// The uncached scan behind [`stdlib_structure_defs`].
fn scan_stdlib_structure_defs() -> std::collections::BTreeSet<String> {
    let mut defs = std::collections::BTreeSet::new();
    let dir = std::path::Path::new(STDLIB_DIR);
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| {
        panic!(
            "ctor_conformance_corpus_survey: cannot read the stdlib dir {}: {e}",
            dir.display()
        )
    });
    for entry in entries {
        let entry = entry.unwrap_or_else(|e| {
            panic!(
                "ctor_conformance_corpus_survey: cannot read an entry of the stdlib dir \
                 {}: {e}. A dropped entry would silently shrink the known-def set and \
                 demote real ctor sites to `UnresolvedDef`.",
                dir.display()
            )
        });
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ri") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "ctor_conformance_corpus_survey: cannot read the stdlib module {}: {e}. \
                 An unreadable stdlib file would silently drop its defs from the \
                 known-def set and demote every site constructing one of them to \
                 `UnresolvedDef`.",
                path.display()
            )
        });
        collect_structure_defs_into(&source, &mut defs);
    }
    defs
}

/// Every structure def declared by an FEA stdlib module, scanned once.
fn fea_owned_defs() -> &'static std::collections::BTreeSet<String> {
    static DEFS: std::sync::OnceLock<std::collections::BTreeSet<String>> =
        std::sync::OnceLock::new();
    DEFS.get_or_init(|| scan_structure_defs(std::path::Path::new(STDLIB_DIR), FEA_STDLIB_MODULES))
}

/// The D9 fix-forward class governing a site whose structure def is `def`.
///
/// Mechanizes exactly the half of D9 that IS decidable — whether the def is
/// FEA-owned, hence call-site-changes-only with field-type flips deferred to
/// v0.6.
///
/// `Owner::NonFea` — the only bucket γ should size as actionable — is reached
/// ONLY when `def` is a name that `structure_defs` actually declares. Everything
/// else routes to a triage bucket: an unrecovered name to [`Owner::Unknown`], a
/// recovered name that is not a declared structure def to
/// [`Owner::UnresolvedDef`]. Both directions of that guard matter, because
/// [`ctor_type_name_at`] recovers any identifier followed by `(` and therefore
/// cannot tell a ctor from a function call; guessing in the touchable direction
/// is the one classification error with a real cost.
///
/// # Approximation: one global namespace, not per-file module scope
///
/// `structure_defs` is accumulated across the WHOLE corpus plus the stdlib, with
/// no module scoping, so `Owner::NonFea` means "some file declares this name" —
/// not "the file this site sits in can see that declaration". That over-includes
/// in the touchable direction: a site constructing `Widget` is called actionable
/// whenever ANY unrelated corpus member declares `structure def Widget`. The FEA
/// direction has the same shape but fails conservatively (over-deferring costs γ
/// sizing accuracy, never a wrong edit), so only the `NonFea` direction is
/// exposed. Removing it needs each member's import graph resolved — more
/// machinery than a one-shot snapshot warrants — so the renderer STATES the
/// approximation in the artifact's named limitations, and the `non-FEA` group
/// blurb points γ at it, rather than leaving it for a reader to infer.
fn d9_owner(
    def: Option<&str>,
    fea_defs: &std::collections::BTreeSet<String>,
    structure_defs: &std::collections::BTreeSet<String>,
) -> Owner {
    match def {
        None => Owner::Unknown,
        Some(name) if fea_defs.contains(name) => Owner::FeaDeferredToV06,
        Some(name) if structure_defs.contains(name) => Owner::NonFea,
        Some(_) => Owner::UnresolvedDef,
    }
}

/// The neutral hint used when no (expected, found) pattern is recognised.
const NO_HINT: &str = "no mechanical hint — γ per-case judgment";

/// Every string `reify_core::Type` renders for a selector-typed field.
///
/// Read off the Display impls, NOT guessed: `SelectorKind`'s four arms render
/// `<Kind>Selector` (`crates/reify-core/src/ty.rs`), and `Type::AnySelector`
/// renders the bare `Selector`. There is no `Selector(Face)` form anywhere —
/// an earlier draft of this file matched exactly that, and so silently gave
/// NO_HINT to every real selector site including the D3 String→selector case
/// that is the PRD's headline illegality.
const SELECTOR_TYPE_RENDERINGS: &[&str] = &[
    "Selector",
    "FaceSelector",
    "EdgeSelector",
    "VertexSelector",
    "BodySelector",
];

/// Whether `ty` renders as a selector-typed field.
fn is_selector_type(ty: &str) -> bool {
    SELECTOR_TYPE_RENDERINGS.contains(&ty)
}

/// The `Type` Display prefixes that introduce a coordinate pose.
///
/// Each is ALWAYS followed by the dimension digits in the real Display impl,
/// and then by NOTHING or by `<` — `Frame3`, `Transform3`, `Point3<Length>`
/// (`crates/reify-core/src/ty.rs`, the three `write!` arms). Those three shapes
/// are the entire pose surface; [`is_pose_type`] admits exactly them.
const POSE_TYPE_PREFIXES: &[&str] = &["Frame", "Transform", "Point"];

/// Whether `ty` renders as a coordinate pose rather than a region target.
///
/// The dimension is REQUIRED, and so is what comes after it — the remainder
/// past the prefix must be a dimension and NOTHING ELSE.
///
/// Why the predicate is this tight. `Type::StructureRef(name)` Displays as the
/// bare struct name, so any struct whose name merely STARTS like a pose reaches
/// this function as a candidate:
///
/// * A bare-prefix match would call the real defs `PointLoad` and `PointCloud`
///   poses — and `PointLoad` is the one FEA def PRD §4 D9 singles out by name.
/// * A digit-only guard (`rest` starts with an ASCII digit) is not enough
///   either: `Point3D`, `Point2Ref` and `Frame4Bar` are all perfectly legal
///   struct names carrying a digit right after the prefix. None exists in the
///   corpus today, so that was latent rather than live — but a def named
///   `Point3D` landing later would silently start collecting a pose verdict.
///
/// Either miss puts a confidently WRONG remedy string ("a pose locates a datum,
/// it does not name a region target") on rows inside the do-not-touch
/// partition, which is worse for γ's sizing than the neutral fallback. Both
/// boundaries are pinned in
/// [`selector_type_renderings_match_what_reify_core_actually_displays`].
fn is_pose_type(ty: &str) -> bool {
    POSE_TYPE_PREFIXES.iter().any(|p| {
        ty.strip_prefix(p).is_some_and(|rest| {
            let after_dim = rest.trim_start_matches(|c: char| c.is_ascii_digit());
            // At least one digit consumed, and what follows is either the end of
            // the string (`Frame3`) or the quantity parameter (`Point3<Length>`).
            after_dim.len() < rest.len() && (after_dim.is_empty() || after_dim.starts_with('<'))
        })
    })
}

/// Whether `ty` renders as a DIMENSIONED scalar (`Scalar[…]`, not bare `Real`).
fn is_dimensioned_scalar(ty: &str) -> bool {
    ty.starts_with("Scalar[")
}

/// An ADVISORY remedy hint, derived purely and deterministically from the
/// (expected, found) type pair.
///
/// This is NOT a D9 ruling. The PRD defines the split between class (1) call-
/// site bug and class (2) wrong declared field type as "per-case judgment …
/// whichever is the actual bug" and assigns it to γ; fabricating a verdict here
/// would be exactly the hand-derivation β is forbidden. Every string below
/// therefore describes what the *shape* of the mismatch suggests, and the
/// artifact's column header says "advisory".
///
/// An unrecognised pair — or one with a missing half — gets [`NO_HINT`], never
/// an invented remedy.
fn remedy_hint(expected: Option<&str>, found: Option<&str>) -> String {
    let (Some(expected), Some(found)) = (expected, found) else {
        return NO_HINT.to_owned();
    };
    if is_selector_type(expected) && found == "String" {
        // D3: implicit String → selector-typed field is newly ILLEGAL; callers
        // move to typed ctors.
        return "selector field given a string — typed ctor such as face(b, \"x_max\") \
                or vertex(b, \"tip\") is the usual replacement"
            .to_owned();
    }
    if is_selector_type(expected) && is_pose_type(found) {
        // D2 pose-vs-set: the fixed hint substring task 4833's fixtures assert.
        return "selector field given a coordinate pose — a pose locates a datum, \
                it does not name a region target"
            .to_owned();
    }
    if is_dimensioned_scalar(expected) && (found == "Real" || found == "Int") {
        // D4-6 dimensioned-scalar migration family.
        return "dimensioned scalar field given a bare number — a dimensioned \
                literal (e.g. 1m/s) is the usual replacement"
            .to_owned();
    }
    if expected == "String" && (found == "Int" || found == "Real" || found == "Bool") {
        return "string field given a non-string literal".to_owned();
    }
    NO_HINT.to_owned()
}

#[test]
fn scan_structure_defs_reads_only_the_listed_modules() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("fea.ri"),
        "module std.fea\nstructure def Alpha { param a : Real }\nstructure def Beta { }\n",
    )
    .expect("write fea.ri");
    std::fs::write(
        dir.path().join("joints.ri"),
        "module std.joints\nstructure def Gamma { }\n",
    )
    .expect("write joints.ri");

    let defs = scan_structure_defs(dir.path(), &["fea"]);
    assert_eq!(
        defs.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["Alpha", "Beta"],
        "only the LISTED module's defs may enter the FEA partition"
    );
    assert!(
        !defs.contains("Gamma"),
        "an unlisted module's defs must not be classified as FEA-owned"
    );
}

#[test]
fn scan_structure_defs_ignores_structure_def_prose_inside_comments() {
    // Measured, not hypothetical: `stdlib/fea_multi_case.ri` line 292 contains
    // the comment "// is a strict relaxation for PointLoad (its structure def
    // already declares". A naive substring scan harvests `already` as a def
    // name and would mis-classify any site whose def is literally named that.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("fea.ri"),
        "module std.fea\n\
         // its structure def already declares point and force\n\
         structure def Real1 { }\n\
             structure def Indented { }\n",
    )
    .expect("write");

    let defs = scan_structure_defs(dir.path(), &["fea"]);
    assert!(
        !defs.contains("already"),
        "prose inside a comment must not enter the def set, got {defs:?}"
    );
    assert!(defs.contains("Real1"), "a real column-0 def must be found");
}

#[test]
#[should_panic(expected = "fea_nonexistent")]
fn scan_structure_defs_panics_when_a_listed_module_is_missing() {
    // A stdlib rename must not silently EMPTY the do-not-touch partition and
    // mis-classify every deferred site as touchable. Fail loud instead.
    let dir = tempfile::tempdir().expect("tempdir");
    let _ = scan_structure_defs(dir.path(), &["fea_nonexistent"]);
}

#[test]
fn fea_owned_defs_scans_the_real_stdlib() {
    let defs = fea_owned_defs();
    assert!(
        !defs.is_empty(),
        "the real FEA stdlib declares structure defs"
    );
    for expected in ["PointLoad", "FixedSupport", "LoadCase", "PressureLoad"] {
        assert!(
            defs.contains(expected),
            "'{expected}' is declared in stdlib/fea_multi_case.ri and must be FEA-owned; got {} defs",
            defs.len()
        );
    }
    assert!(
        !defs.contains("already"),
        "the comment-prose false positive must not reach the real scan either"
    );
}

#[test]
fn d9_owner_classifies_fea_non_fea_and_unattributed() {
    let fea = fea_owned_defs();
    let known: std::collections::BTreeSet<String> = ["Widget", "PointLoad"]
        .into_iter()
        .map(str::to_owned)
        .collect();

    assert_eq!(
        d9_owner(Some("PointLoad"), fea, &known),
        Owner::FeaDeferredToV06,
        "D9: FEA defs are call-site-only; field-type flips stay v0.6-owned. FEA \
         ownership outranks the known-def gate, not the other way round"
    );
    assert_eq!(
        d9_owner(Some("Widget"), fea, &known),
        Owner::NonFea,
        "a KNOWN structure def that is not FEA-owned falls under D9's per-case judgment"
    );
    assert_eq!(
        d9_owner(Some("union"), fea, &known),
        Owner::UnresolvedDef,
        "`union` is a stdlib FUNCTION, not a structure def — recovery cannot tell the \
         two apart from `ident(`, so a name that resolves to no declaration must NOT \
         be sized into γ's actionable pile"
    );
    assert_eq!(
        d9_owner(None, fea, &known),
        Owner::Unknown,
        "an unattributable site must NEVER silently default into the touchable bucket"
    );
}

#[test]
fn structure_def_scanner_reads_the_visibility_prefixes() {
    let mut defs = std::collections::BTreeSet::new();
    collect_structure_defs_into(
        "pub structure def Actuator { }\n\
         priv structure def Hidden { }\n\
         structure def Plain { }\n\
         // structure def Commented { }\n\
         \x20   structure def Indented { }\n\
         let x = 1 // its structure def already declares y\n",
        &mut defs,
    );
    let got: Vec<&str> = defs.iter().map(String::as_str).collect();
    assert_eq!(
        got,
        vec!["Actuator", "Hidden", "Plain"],
        "`pub`/`priv` prefixes are part of the declaration grammar (10 `pub structure \
         def` sites in the tracked corpus); comments, indented prose and mid-line \
         mentions are not declarations"
    );
}

#[test]
fn stdlib_structure_defs_is_a_superset_of_the_fea_partition() {
    let all = stdlib_structure_defs();
    let fea = fea_owned_defs();
    assert!(
        !all.is_empty(),
        "the stdlib def scan must not be empty — an empty known-def set would demote \
         EVERY row to `UnresolvedDef` and empty γ's actionable group"
    );
    let missing: Vec<&String> = fea.iter().filter(|d| !all.contains(*d)).collect();
    assert!(
        missing.is_empty(),
        "the FEA modules are stdlib files, so every FEA def must also be found by the \
         whole-stdlib scan; missing: {missing:?}"
    );
}

#[test]
fn remedy_hint_is_a_pure_deterministic_function_of_the_type_pair() {
    // Same input -> same output, no I/O, no ordering dependence.
    let a = remedy_hint(Some("FaceSelector"), Some("String"));
    let b = remedy_hint(Some("FaceSelector"), Some("String"));
    assert_eq!(a, b, "remedy_hint must be deterministic");

    // Distinct recognised pairs map to DISTINCT fixed strings.
    //
    // `Frame3`, NOT `Frame(3)`: `Type::Frame(3)` Displays as `Frame3`
    // (`crates/reify-core/src/ty.rs`, pinned by that crate's own test) and
    // `is_pose_type` requires the dimension digit immediately after the prefix.
    // An earlier draft passed `"Frame(3)"` here, which yields `(3)` after the
    // prefix strip and is therefore NOT a pose — so `pose_at_selector` silently
    // held NO_HINT and every assertion below about it was vacuous.
    let string_at_selector = remedy_hint(Some("FaceSelector"), Some("String"));
    let pose_at_selector = remedy_hint(Some("FaceSelector"), Some("Frame3"));
    let bare_at_dimensioned = remedy_hint(Some("Scalar[m·s^-1]"), Some("Real"));
    assert_ne!(string_at_selector, pose_at_selector);
    assert_ne!(string_at_selector, bare_at_dimensioned);
    assert_ne!(pose_at_selector, bare_at_dimensioned);
    for h in [&string_at_selector, &pose_at_selector, &bare_at_dimensioned] {
        assert!(!h.is_empty(), "a recognised pair must produce a hint");
    }

    // An unrecognised pair, and a pair with a missing half, get a NEUTRAL
    // string — never an invented remedy.
    let neutral = remedy_hint(None, None);
    assert_eq!(remedy_hint(Some("Widget"), Some("Gadget")), neutral);
    assert_eq!(remedy_hint(Some("FaceSelector"), None), neutral);
    assert_eq!(remedy_hint(None, Some("String")), neutral);
    assert_ne!(
        neutral, string_at_selector,
        "the neutral string must be distinguishable from a real hint"
    );
    assert_ne!(
        neutral, pose_at_selector,
        "the D2 pose arm must produce a REAL hint, not the neutral fallback — \
         without this the pose fixture above can silently degrade to NO_HINT again \
         and every `assert_ne!` naming it still passes"
    );
}

#[test]
fn selector_type_renderings_match_what_reify_core_actually_displays() {
    use reify_core::Type;
    use reify_core::ty::SelectorKind;

    // Pin the table against the REAL Display impl by constructing types and
    // rendering them, rather than hand-transcribing wire forms. An earlier
    // draft matched `Selector(Face)` — a string the compiler never emits — so
    // every real selector site fell through to the neutral hint. Constructing
    // the values here means a Display rename goes RED instead of silently
    // re-emptying the selector arm.
    for kind in [
        SelectorKind::Face,
        SelectorKind::Edge,
        SelectorKind::Vertex,
        SelectorKind::Body,
    ] {
        let rendered = Type::Selector(kind).to_string();
        assert!(
            is_selector_type(&rendered),
            "Type::Selector({kind:?}) renders as {rendered:?}, which is_selector_type \
             does not recognise"
        );
    }
    let any = Type::AnySelector.to_string();
    assert!(
        is_selector_type(&any),
        "Type::AnySelector renders as {any:?}, which is_selector_type does not recognise"
    );

    // And the D3 case end-to-end: a String at a selector-typed field must get
    // the typed-ctor hint, not the neutral fallback.
    let hint = remedy_hint(Some(&any), Some("String"));
    assert_ne!(
        hint, NO_HINT,
        "the D3 String→selector case is the PRD's headline illegality; it must \
         carry a hint"
    );
    assert!(
        hint.contains("face(b"),
        "the hint names the typed-ctor replacement"
    );

    // Pose Display forms are `Frame3` / `Transform3` / `Point3<Length>`.
    for pose in [
        Type::Frame(3).to_string(),
        Type::Transform(3).to_string(),
        Type::point3(Type::length()).to_string(),
    ] {
        assert!(
            is_pose_type(&pose),
            "{pose:?} must be recognised as a coordinate pose"
        );
        assert_ne!(
            remedy_hint(Some(&any), Some(&pose)),
            NO_HINT,
            "the D2 pose-vs-set case must carry a hint for {pose:?}"
        );
    }

    // …and the NEGATIVE half, which the true-positive loop above cannot catch:
    // `Type::StructureRef(name)` Displays as the BARE struct name, so a
    // prefix-only `is_pose_type` would call these poses. `PointLoad` is a real
    // FEA def (PRD §4 D9 names it), `PointCloud` is a real def in this tree, and
    // both would then carry the D2 "a pose locates a datum" hint — a confidently
    // wrong remedy inside the do-not-touch partition.
    //
    // The last three pin the OTHER boundary, one step in from the bare-prefix
    // one: a digit-only guard admits every one of them. `Point3D`, `Point2Ref`
    // and `Frame4Bar` are legal struct names that carry a digit immediately
    // after a pose prefix, and no such def exists in the corpus today — so the
    // bug would have been latent until one landed, and then silent. A pose's
    // dimension is followed by end-of-string or `<`, never by more name.
    for not_a_pose in [
        "PointLoad",
        "PointCloud",
        "Framework",
        "Transformer",
        "Point3D",
        "Point2Ref",
        "Frame4Bar",
    ] {
        let rendered = Type::StructureRef(not_a_pose.into()).to_string();
        assert_eq!(
            rendered, not_a_pose,
            "Type::StructureRef must still Display as the bare struct name; if that \
             changed, this negative case is testing the wrong string"
        );
        assert!(
            !is_pose_type(&rendered),
            "{rendered:?} is a structure ref, not a coordinate pose — a bare-prefix \
             match here puts a false D2 remedy hint on real rows"
        );
        assert_eq!(
            remedy_hint(Some(&any), Some(&rendered)),
            NO_HINT,
            "a structure ref at a selector-typed field has no mechanical remedy; the \
             neutral fallback is the honest answer"
        );
    }
}

// ─── step 9/10: end-to-end sweep over a synthetic mini-corpus ────────────────

/// The result of one pass over a corpus.
///
/// `surveyed + not_surveyed.len() == total` is an invariant: every member is
/// accounted for. The house "no silent caps" rule applies directly here — a
/// bounded sweep must state what it dropped, or the artifact reads as full
/// coverage and under-sizes γ.
#[derive(Debug, Default)]
struct SurveyRun {
    /// Every member handed in — the coverage denominator.
    total: usize,
    /// Members that reached the compile phase and contributed their sites.
    surveyed: usize,
    /// `(path, reason)` for members that contributed NO sites at all.
    /// Reasons: `read-error`, `parse-error`.
    not_surveyed: Vec<(String, String)>,
    /// `(path, reason)` for members that WERE surveyed but whose compile also
    /// produced Error-severity diagnostics — their ctor sites are collected,
    /// but coverage of that file may be partial. Reason: `compile-error`.
    ///
    /// A separate bucket from `not_surveyed` on purpose: `compile_with_stdlib`
    /// is the SINGLE-FILE path (`reify check` instead uses
    /// `module_dag::compile_entry_with_stdlib_cfg_checked`, which follows
    /// `#cfg`-gated user imports), so every multi-module corpus member — the
    /// `examples/module_visibility/consumer.ri` class — lands here. Calling
    /// those "not surveyed" would understate coverage; calling them fully
    /// surveyed would overstate it. Naming them is the honest third option.
    partial: Vec<(String, String)>,
    /// Every ctor-conformance site found, sorted `(file, line, field)`.
    sites: Vec<SurveySite>,
}

/// Sweep `rel_paths` (resolved against `root`) and collect every
/// ctor-conformance site.
///
/// Mirrors `examples_smoke.rs`'s `ctor_conformance_one` — read →
/// `parse_with_stdlib(&source, ModulePath::single(stem))` →
/// `compile_with_stdlib` → filter `compiled.diagnostics` by
/// `is_ctor_conformance_code` — so the survey and the landed α corpus gate
/// cannot disagree about what a ctor-conformance site IS.
///
/// Two deliberate differences from that gate:
/// 1. The root widens from `examples/` to whatever corpus is handed in.
/// 2. Read and parse failures are RECORDED rather than panicked-on or silently
///    `return`ed. The non-examples corpus contains many intentionally
///    unparseable fixtures, and omitting them would inflate apparent coverage.
///
/// D9 owner assignment is a SECOND pass, after the loop: a site in the first
/// swept file may construct a def declared in the last one, so the known-def set
/// has to be complete before any row is classified. Classifying inline would
/// make a row's owner depend on the order members were handed in — the exact
/// non-determinism the artifact's byte-reproducibility rules out.
fn survey_corpus(root: &std::path::Path, rel_paths: &[String]) -> SurveyRun {
    use reify_compiler::{compile_with_stdlib, parse_with_stdlib};
    use reify_core::{ModulePath, Severity};

    let fea = fea_owned_defs();
    // Seeded with the stdlib so a site constructing a stdlib def resolves even
    // when the declaring stdlib file is not part of the corpus handed in; every
    // swept member then contributes its own declarations below.
    let mut structure_defs = stdlib_structure_defs().clone();
    let mut run = SurveyRun {
        total: rel_paths.len(),
        ..SurveyRun::default()
    };

    for rel in rel_paths {
        let path = root.join(rel);
        let Ok(source) = std::fs::read_to_string(&path) else {
            run.not_surveyed
                .push((rel.clone(), "read-error".to_owned()));
            continue;
        };
        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        // Declarations are harvested from the raw text BEFORE the parse gate:
        // a member that fails to parse can still legitimately declare a def that
        // another member constructs, and dropping it would demote that other
        // member's rows to `UnresolvedDef` for no reason.
        collect_structure_defs_into(&source, &mut structure_defs);

        let parsed = parse_with_stdlib(&source, ModulePath::single(&stem));
        if !parsed.errors.is_empty() {
            run.not_surveyed
                .push((rel.clone(), "parse-error".to_owned()));
            continue;
        }

        let compiled = compile_with_stdlib(&parsed);
        run.surveyed += 1;
        if compiled
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
        {
            run.partial.push((rel.clone(), "compile-error".to_owned()));
        }
        for d in compiled
            .diagnostics
            .iter()
            .filter(|d| is_ctor_conformance_code(d.code))
        {
            let Some(site) = survey_site_from_diagnostic(rel, &source, d) else {
                continue;
            };
            run.sites.push(site);
        }
    }

    // Second pass: every declaration in the corpus is now known.
    for site in &mut run.sites {
        site.owner = d9_owner(site.def.as_deref(), fea, &structure_defs);
    }

    // Total order, so the artifact is byte-reproducible regardless of the order
    // members were handed in. `code` and `message` break the remaining ties so
    // two sites at the same (file, line, field) still sort deterministically.
    run.sites.sort_by(|a, b| {
        (&a.file, a.line, &a.field, &a.code, &a.message)
            .cmp(&(&b.file, b.line, &b.field, &b.code, &b.message))
    });
    run.not_surveyed.sort();
    run.partial.sort();
    run
}

/// The known-WARNING member: PRD §7 boundary-test row 2, reused verbatim from
/// `struct_ctor_field_conformance_tests.rs`'s `SOURCE_ROW2_VALUE_CELL_STRING`.
///
/// Using α's own landed fixture means the end-to-end test asserts against a
/// site shape the compiler is ALREADY proven to emit — the premise is verified
/// live on `main`, not guessed.
#[cfg(test)]
const SYNTH_WARNS: &str = "module test.row2\n\
     structure def Widget { param label : String }\n\
     structure def Root {\n\
     \x20   let x = Widget(label: 42)\n\
     }\n";

/// The known-CLEAN member: the same source with a conforming argument.
#[cfg(test)]
const SYNTH_CLEAN: &str = "module test.clean\n\
     structure def Widget { param label : String }\n\
     structure def Root {\n\
     \x20   let x = Widget(label: \"ok\")\n\
     }\n";

/// The known-UNPARSEABLE member. The tracked corpus really does contain
/// deliberately-unparseable negative fixtures and tree-sitter parser-corpus
/// inputs, so the sweep must record them rather than panic.
#[cfg(test)]
const SYNTH_BROKEN: &str = "module test.broken\n((( this is not reify at all ]]] §§§\n";

/// The known-COMPILE-ERROR member: parses cleanly, then emits an Error-severity
/// diagnostic (`unresolved name: no_such_binding`).
///
/// This is the `partial` bucket's only coverage. Roughly a tenth of the tracked
/// corpus lands there — every multi-module file the single-file
/// `compile_with_stdlib` path cannot resolve; the artifact header carries the
/// exact figure, which drifts as the corpus grows — yet without this member the
/// synthetic sweep never populates `run.partial` at all, and a regression that
/// stopped filling it (or that moved compile-error files into `not_surveyed`,
/// breaking the coverage arithmetic) would go undetected while every other test
/// stayed green.
#[cfg(test)]
const SYNTH_COMPILE_ERROR: &str = "module test.compile_error\n\
     structure def Root {\n\
     \x20   let x = no_such_binding + 1\n\
     }\n";

// ─── prose-contract members: the extractors, against the LIVE emitters ───────
//
// `SYNTH_WARNS` above pins `ArgTypeMismatch`'s `argument '<f>'` /
// `expected '<X>', got '<Y>'` shapes end-to-end. Every OTHER admitted wording
// was pinned only against `synth(...)` fixtures whose message strings are
// hand-written in this file — a presumed copy of what the emitters produce, not
// a measurement of it. An emitter rewording would leave all of those green
// while `def_of_diagnostic` / `field_of_message` silently stopped recovering on
// real input, and the committed artifact carries zero ε rows today, so nothing
// else would surface it either.
//
// This is the same failure the module already fixed once for the type table:
// `selector_type_renderings_match_what_reify_core_actually_displays` pins
// `is_selector_type` by CONSTRUCTING `reify_core::Type` values after an earlier
// draft matched a `Selector(Face)` string the compiler never emits.
//
// These three members close the gap for the remaining shapes at the same
// near-zero cost — three small files through the real parse→compile pipeline.
// They are deliberately a SEPARATE corpus rather than extra `synth_corpus()`
// members: `survey_corpus_finds_the_known_warning_site_with_every_column_resolved`
// asserts "exactly one ctor-conformance site across the mini-corpus", and the
// coverage-arithmetic tests assert `run.total == 4`. Diluting those to carry
// prose coverage would weaken guards that exist for a different reason.

/// ε `CtorUnknownField`, live: the `in call to '<Def>'` def prose and the
/// `unknown named argument '<f>'` field prose in one message.
#[cfg(test)]
const PROSE_CTOR_UNKNOWN_FIELD: &str = "module test.prose_unknown_field\n\
     structure def Widget { param label : String }\n\
     structure def Root {\n\
     \x20   let x = Widget(nosuchfield: \"v\")\n\
     }\n";

/// ε `CtorArity`, live: the `E_CTOR_ARITY: <Def>() expects …` def prose. Its
/// wording names no param, so the FIELD column must stay `None` — the one
/// admitted code for which that is the correct answer rather than a miss.
#[cfg(test)]
const PROSE_CTOR_ARITY: &str = "module test.prose_arity\n\
     structure def Widget { param label : String }\n\
     structure def Root {\n\
     \x20   let x = Widget(\"a\", \"b\")\n\
     }\n";

/// `TypeNotConformingToTrait`'s `required by param '<f>'` shape, live.
///
/// The `sub =` binding and the empty `trait Fastener {}` are both load-bearing:
/// this is the shape `m9_error_cases.rs`'s `type_does_not_conform_to_trait`
/// proves the compiler emits, and a `let` binding of a trait-param ctor does
/// NOT reach the check (measured — an earlier draft of this fixture produced no
/// diagnostic at all and would have been a silently vacuous test).
#[cfg(test)]
const PROSE_REQUIRED_BY_PARAM: &str = "module test.prose_required_by_param\n\
     trait Fastener {}\n\
     structure def Bolt { param dia : Real = 3.0 }\n\
     structure def Holder { param part : Fastener }\n\
     structure def Root { sub h = Holder(part: Bolt()) }\n";

/// Write the three prose-contract members into a temp dir and sweep them.
#[cfg(test)]
fn prose_contract_run() -> SurveyRun {
    let dir = tempfile::tempdir().expect("tempdir");
    let members = [
        ("arity.ri", PROSE_CTOR_ARITY),
        ("required_by_param.ri", PROSE_REQUIRED_BY_PARAM),
        ("unknown_field.ri", PROSE_CTOR_UNKNOWN_FIELD),
    ];
    for (name, source) in members {
        std::fs::write(dir.path().join(name), source).expect("write prose-contract member");
    }
    let paths: Vec<String> = members.iter().map(|(n, _)| (*n).to_owned()).collect();
    survey_corpus(dir.path(), &paths)
}

#[test]
fn epsilon_and_required_by_param_prose_extractors_hold_against_the_live_emitters() {
    let run = prose_contract_run();

    // Every member must actually COMPILE and yield its site. A fixture that
    // stopped reaching its emitter would otherwise make each assertion below
    // vacuous rather than red.
    assert_eq!(
        run.not_surveyed,
        Vec::new(),
        "every prose-contract member must parse; a fixture that stopped compiling \
         would make this whole test vacuous"
    );
    assert_eq!(
        run.sites.len(),
        3,
        "one site per prose-contract member — a missing one means that emitter no \
         longer produces the shape this test claims to pin: {:#?}",
        run.sites
    );

    let site = |file: &str| {
        run.sites
            .iter()
            .find(|s| s.file == file)
            .unwrap_or_else(|| panic!("no site for {file}; got {:#?}", run.sites))
    };

    // ε `CtorUnknownField`: BOTH prose extractors, measured.
    let unknown = site("unknown_field.ri");
    assert_eq!(unknown.code, "CtorUnknownField");
    assert_eq!(
        unknown.def.as_deref(),
        Some("Widget"),
        "the `in call to '<Def>'` prose must still yield the def"
    );
    assert_eq!(
        unknown.def_origin,
        DefOrigin::DiagnosticProse,
        "the def must come from the PROSE path, not the call-site anchor — if the \
         wording drifts, this is where it shows up"
    );
    assert_eq!(
        unknown.field.as_deref(),
        Some("nosuchfield"),
        "the `unknown named argument '<f>'` prose must still yield the field"
    );

    // ε `CtorArity`: def from prose, and field CORRECTLY absent.
    let arity = site("arity.ri");
    assert_eq!(arity.code, "CtorArity");
    assert_eq!(
        arity.def.as_deref(),
        Some("Widget"),
        "the `E_CTOR_ARITY: <Def>()` prose must still yield the def"
    );
    assert_eq!(arity.def_origin, DefOrigin::DiagnosticProse);
    assert_eq!(
        arity.field, None,
        "this wording names no param; a field here would be a fabrication, not a \
         recovery"
    );

    // `required by param '<f>'` — reached ONLY through `field_of_message`'s
    // `or_else` fallback, because the message carries no `argument '`. This is
    // the live-emitter half of the fallback-liveness contract that
    // `an_empty_quoted_token_is_a_miss_so_the_fallback_prefix_is_still_consulted`
    // pins structurally.
    let required = site("required_by_param.ri");
    assert_eq!(required.code, "TypeNotConformingToTrait");
    assert!(
        !required.message.contains(ARG_PREFIX),
        "premise: this shape must NOT carry `argument '`, or it would be recovered \
         by the first prefix and the fallback would go untested; got {:?}",
        required.message
    );
    assert_eq!(
        required.field.as_deref(),
        Some("part"),
        "the `required by param '<f>'` FALLBACK must still yield the field"
    );
    // Measured, and stated rather than editorialised: the label anchors at the
    // ARGUMENT's ctor (`Bolt()`), so the call-site anchor recovers the argument
    // type, not the receiving def (`Holder`). That is the survey's documented
    // "def is whatever identifier sits at the anchor" caveat, observed live.
    assert_eq!(required.def.as_deref(), Some("Bolt"));
    assert_eq!(required.def_origin, DefOrigin::CallSiteAnchor);
}

/// Write the three synthetic members into a temp dir and return `(dir, paths)`.
#[cfg(test)]
fn synth_corpus() -> (tempfile::TempDir, Vec<String>) {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, source) in [
        ("warns.ri", SYNTH_WARNS),
        ("clean.ri", SYNTH_CLEAN),
        ("broken.ri", SYNTH_BROKEN),
        ("compile_error.ri", SYNTH_COMPILE_ERROR),
    ] {
        std::fs::write(dir.path().join(name), source).expect("write synthetic member");
    }
    let paths = vec![
        "broken.ri".to_owned(),
        "clean.ri".to_owned(),
        "compile_error.ri".to_owned(),
        "warns.ri".to_owned(),
    ];
    (dir, paths)
}

#[test]
fn survey_corpus_finds_the_known_warning_site_with_every_column_resolved() {
    let (dir, paths) = synth_corpus();
    let run = survey_corpus(dir.path(), &paths);

    assert_eq!(
        run.sites.len(),
        1,
        "exactly one ctor-conformance site across the mini-corpus, got: {:#?}",
        run.sites
    );
    let site = &run.sites[0];
    assert_eq!(site.file, "warns.ri");
    assert_eq!(site.code, "ArgTypeMismatch");
    assert_eq!(
        site.severity, "Warning",
        "α's knob is Warning — the sweep must report what it MEASURED, never assume Error"
    );
    assert_eq!(site.field.as_deref(), Some("label"));
    assert_eq!(site.def.as_deref(), Some("Widget"));
    assert_eq!(site.expected.as_deref(), Some("String"));
    assert_eq!(site.found.as_deref(), Some("Int"));
    assert_eq!(
        site.line, 4,
        "the ctor is on line 4 of the fixture; got line {} for {:?}",
        site.line, site.message
    );
    assert_eq!(
        site.owner,
        Owner::NonFea,
        "`Widget` is not an FEA stdlib def — the sweep must CLASSIFY, not leave Unknown"
    );
}

#[test]
fn survey_corpus_records_unsurveyable_members_instead_of_dropping_them() {
    let (dir, paths) = synth_corpus();
    let run = survey_corpus(dir.path(), &paths);

    assert_eq!(run.total, 4, "the denominator is every file handed in");
    let broken: Vec<&(String, String)> = run
        .not_surveyed
        .iter()
        .filter(|(f, _)| f == "broken.ri")
        .collect();
    assert_eq!(
        broken.len(),
        1,
        "the unparseable member must be RECORDED, not silently dropped: {:#?}",
        run.not_surveyed
    );
    assert_eq!(
        broken[0].1, "parse-error",
        "its reason must name the phase that failed"
    );

    // "No silent caps": the coverage denominator has to be visible, or γ is
    // sized against a survey that reads as full coverage but is not.
    assert_eq!(
        run.surveyed + run.not_surveyed.len(),
        run.total,
        "surveyed + not_surveyed must account for every corpus member"
    );
    assert_eq!(
        run.surveyed, 3,
        "the three parseable members are surveyed — including the one that then \
         failed to COMPILE, which is partial coverage, not zero coverage"
    );
}

#[test]
fn survey_corpus_records_a_compile_error_member_as_partial_not_missing() {
    let (dir, paths) = synth_corpus();
    let run = survey_corpus(dir.path(), &paths);

    assert_eq!(
        run.partial,
        vec![("compile_error.ri".to_owned(), "compile-error".to_owned())],
        "a member that PARSES and then emits an Error-severity diagnostic belongs in \
         `partial` with its reason named. Roughly a tenth of the tracked corpus lands \
         here — every multi-module file the single-file `compile_with_stdlib` path \
         cannot resolve; the artifact header carries the exact count — so an empty \
         `partial` on the real corpus would be a silent coverage overstatement: {:#?}",
        run.partial
    );

    // The three-way split has to stay consistent, or the artifact's coverage
    // arithmetic (`surveyed + not_surveyed == total`) stops adding up.
    assert!(
        run.not_surveyed
            .iter()
            .all(|(f, _)| f != "compile_error.ri"),
        "a partially-surveyed member must NOT also be listed as not-surveyed — that \
         would double-count it and break the coverage denominator: {:#?}",
        run.not_surveyed
    );
    assert_eq!(
        run.surveyed + run.not_surveyed.len(),
        run.total,
        "`partial` is a QUALIFIER on surveyed members, never a fourth disjoint bucket"
    );
    assert!(
        run.partial.iter().all(|(f, _)| f != "broken.ri"),
        "a member that never reached the compile phase cannot be `partial`: {:#?}",
        run.partial
    );
}

#[test]
fn survey_corpus_does_not_leak_prelude_diagnostics_into_every_file() {
    // If stdlib-prelude diagnostics were re-attributed to each swept file, the
    // artifact would inflate ~660x and be worthless. Two distinct files, each
    // yielding ONLY its own sites, is the cheap pin on that.
    let (dir, paths) = synth_corpus();
    let run = survey_corpus(dir.path(), &paths);

    let clean_sites = run.sites.iter().filter(|s| s.file == "clean.ri").count();
    assert_eq!(
        clean_sites, 0,
        "the conforming member must contribute ZERO sites; prelude leakage would \
         give it the same site count as every other file"
    );

    // Sweeping the clean member ALONE must likewise be empty.
    let solo = survey_corpus(dir.path(), &["clean.ri".to_owned()]);
    assert!(
        solo.sites.is_empty(),
        "a clean file swept alone must yield no sites, got: {:#?}",
        solo.sites
    );
    assert_eq!(solo.surveyed, 1);
    assert_eq!(solo.total, 1);
}

#[test]
fn survey_corpus_records_a_read_error_rather_than_panicking() {
    let (dir, _) = synth_corpus();
    let run = survey_corpus(dir.path(), &["no_such_file.ri".to_owned()]);
    assert_eq!(run.total, 1);
    assert_eq!(run.surveyed, 0);
    assert_eq!(
        run.not_surveyed,
        vec![("no_such_file.ri".to_owned(), "read-error".to_owned())],
        "an unreadable member is recorded with its reason, never a panic that \
         would abort a 660-file sweep"
    );
}

#[test]
fn survey_corpus_orders_sites_deterministically() {
    // Byte-reproducibility of the artifact starts here: the same corpus handed
    // in a different order must produce the same site list.
    let (dir, paths) = synth_corpus();
    let forward = survey_corpus(dir.path(), &paths);
    let mut reversed = paths.clone();
    reversed.reverse();
    let backward = survey_corpus(dir.path(), &reversed);
    assert_eq!(
        forward.sites, backward.sites,
        "site ordering must not depend on the order files are handed in"
    );
}

// ─── γ (task #5305): the files γ migrated to ctor-conformance clean ──────────

/// Repo-relative `.ri` files that task #5305 (γ) migrated to ctor-conformance
/// clean, pinned so a regression is caught on the merge gate instead of only by
/// the `#[ignore]`d corpus generator.
///
/// Deliberately a two-file pin rather than a corpus walk. The corpus-wide
/// assertion lives in [`generate_ctor_conformance_corpus_survey`] and stays
/// `#[ignore]`d because it compiles every tracked `.ri`, ~2.5× the `examples/`
/// walk (`docs/prds/merge-gate-compile-cost.md`). This pin costs one stdlib
/// prelude compile plus these files, so the sites γ actually changed become
/// gate-resident without reversing that landed cost decision.
///
/// Matching is on `DiagnosticCode` IDENTITY via [`is_ctor_conformance_code`],
/// never on message prose. The prose-keyed guard covering the same r3b fixture
/// in `crates/reify-eval-fea-tests/tests/r3b_modal_selector_displacement.rs` is
/// deliberately left alone — it is documented code-AGNOSTIC on purpose and keys
/// on a different pair of params (`alpha` / `beta`).
///
/// Both entries reach the walker by DIFFERENT routes, which is why the pin is
/// worth two files rather than one: the r3b site is a ctor ARG, the
/// `mv-2-priv-param.ri` site is a D8 param DEFAULT
/// (`check_param_default_conformance`). One shared assertion covers both because
/// `survey_corpus` filters on the diagnostic CODE, not on the emitting path.
const CTOR_CONFORMANCE_PINNED_CLEAN: &[&str] = &[
    "tests/prd-gate/fixtures/r3b_displacement_at_selector_grammar.ri",
    "tree-sitter-reify/test/fixtures/mv-2-priv-param.ri",
];

/// Every [`CTOR_CONFORMANCE_PINNED_CLEAN`] file compiles with ZERO
/// ctor-conformance diagnostics.
///
/// Accumulates across files and panics once, so a run names every regressed site
/// rather than stopping at the first — the corpus-wide-visibility principle the
/// sibling gates in this binary already follow.
///
/// Coverage is asserted BEFORE the site count, against every way a pinned file
/// can contribute zero sites WITHOUT being clean:
///
/// * it failed to read or parse — `not_surveyed`. Without that check the pin is
///   satisfied by the file disappearing, or by its grammar breaking.
/// * it was dropped from the corpus handed in — the `surveyed` count is short.
/// * it compiled with Error-severity diagnostics — `partial`. [`survey_corpus`]
///   increments `surveyed` BEFORE it tests for errors, so this third path
///   satisfies both checks above on its own: a pinned file that regresses into a
///   compile error may stop contributing sites entirely, because the conformance
///   walk need never reach the ctor. The generator reports 73 of 684 surveyed
///   members in `partial` today — neither pinned file among them — so the shape
///   is live in the corpus and latent here, not hypothetical.
#[test]
fn pinned_clean_files_emit_no_ctor_conformance_diagnostic() {
    let corpus: Vec<String> = CTOR_CONFORMANCE_PINNED_CLEAN
        .iter()
        .map(|p| (*p).to_owned())
        .collect();
    let run = survey_corpus(std::path::Path::new(WORKSPACE_ROOT), &corpus);

    assert!(
        run.not_surveyed.is_empty(),
        "every pinned file must reach the compile phase, else this pin passes \
         vacuously; unreachable: {:?}",
        run.not_surveyed,
    );
    assert_eq!(
        run.surveyed,
        corpus.len(),
        "all {} pinned file(s) must be surveyed, only {} were",
        corpus.len(),
        run.surveyed,
    );
    assert!(
        run.partial.is_empty(),
        "every pinned file must compile with NO Error-severity diagnostic, else the \
         conformance walk may never reach its ctor and this pin passes vacuously; \
         partial: {:?}\n\n\
         This is a DIFFERENT defect from the site regression reported below and wants a \
         different fix: the file no longer COMPILES. Fix the compile error first — the \
         zero-site result above says nothing about conformance until it does.",
        run.partial,
    );

    let offenders: Vec<String> = run
        .sites
        .iter()
        .map(|s| {
            format!(
                "  {}:{} :: param '{}'  [{} / {}]  {}",
                s.file,
                s.line,
                s.field.as_deref().unwrap_or("—"),
                s.code,
                s.severity,
                s.message,
            )
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "{} ctor-conformance diagnostic(s) in file(s) task #5305 migrated to clean:\n{}\n\n\
         A pinned file carries no waiver and no owner — that is the point of the pin. \
         Either the migration was reverted, or a new un-migrated site was added; fix \
         the site (a ctor argument, or a param default).",
        offenders.len(),
        offenders.join("\n"),
    );
}

// ─── γ (task #5305): the sites γ deferred, with their owners ─────────────────

/// Per-SITE, owner-attributed deferrals for the ctor-conformance warnings that
/// survive γ (task #5305) OUTSIDE `examples/`.
///
/// Each entry is `(repo_relative_path, param_name, owning_task, why)`.
///
/// # These are DELIBERATE before-images. Do not "fix" them.
///
/// Every site below is a committed RED before-image for another PRD, and the
/// conformance violation IS the fixture's content. Several of these files say so
/// in their own header, verbatim: *"This file must EVAL CLEAN (exit 0) today."*
/// `dcr_solver_load_dropped_dimensioned.ri` exists for no other purpose than to
/// show that the units-CORRECT `force: 1000N` contributes exactly ZERO force to
/// `solve_elastic_static` while the bare control contributes 1000 N. Dimension
/// the call site and the measurement it encodes is gone.
///
/// So these are NOT un-migrated call sites that nobody got around to. That
/// distinction is the entire reason this table carries a `why` column.
///
/// # This is a γ RULING, recorded where it can be checked
///
/// PRD §4 D9 assigns the per-case judgment — call-site bug, or wrong declared
/// field type — to γ. γ ruled: two sites were the call site's fault and are
/// fixed in this branch (see [`CTOR_CONFORMANCE_PINNED_CLEAN`]); these eleven
/// are owned elsewhere and are deferred, per the shape
/// `CTOR_CONFORMANCE_GATE_REMEDY` remedy 3 already mandates on main — *"Add a
/// per-SITE entry naming the file, the param and the LIVE task that owns
/// retiring it. Per-site, never per-file, and never without an owner."*
///
/// # Retirement
///
/// Each entry is deleted by its OWNING task's own diff, exactly as #5847 retires
/// the two `CTOR_CONFORMANCE_MIGRATION_DEBT` entries. An entry left behind after
/// its site is retired is caught by the corpus-wide check inside
/// [`generate_ctor_conformance_corpus_survey`], which reports a stale entry and
/// an unexplained warning as two different defects.
///
/// The `why` column's Greek leaf labels are
/// `docs/prds/v0_6/dimension-checked-readers.md`'s, kept alongside the `#NNNN`
/// cite so the attribution stays legible if those cluster tasks are re-split:
/// γ1/ε/η are #6922, γ2/β/ζ are #6941.
///
/// # Sibling of `CTOR_CONFORMANCE_MIGRATION_DEBT`, not a merge of it
///
/// That list is `examples/`-keyed BY CONSTRUCTION: its own doc forbids the
/// repo-relative spelling, and the gate consuming it walks `EXAMPLES_DIR` only.
/// It cannot name a path under `tests/prd-gate/fixtures/` at all. The two tables
/// are joined at exactly one place — the disposition resolver — and
/// [`ctor_conformance_corpus_residual_is_disjoint_from_migration_debt`] keeps
/// them from ever describing the same site.
///
/// # This is NOT `SKIP_SET`
///
/// Nothing here is dropped from any walk. Each entry excuses ONE
/// `(file, param)` pair; every other diagnostic in these files stays unwaived,
/// and a ctor-conformance diagnostic at a different param in the same file is
/// an unexplained warning.
const CTOR_CONFORMANCE_CORPUS_RESIDUAL: &[(&str, &str, &str, &str)] = &[
    (
        "tests/prd-gate/fixtures/curvature_rad_literal.ri",
        "kc",
        "#6179",
        "angle-completion leaf α, boundary row B1: CURVATURE is m^-1 pre-α, so the \
         rad·m^-1 initializer mismatches; the fixture's own header calls that \
         check-time flip α's signal",
    ),
    (
        "tests/prd-gate/fixtures/dcr_load_ctor_dimension_silent.ri",
        "force",
        "#6941",
        "leaf γ2: PointLoad.force is declared `Real` in fea_multi_case.ri, so the \
         units-CORRECT `force: 5000N` warns; γ2 retypes the FIELD, and the call site \
         is already right",
    ),
    (
        "tests/prd-gate/fixtures/dcr_load_ctor_dimension_silent.ri",
        "traction",
        "#6941",
        "leaf γ2: TractionLoad.traction is declared `Real` in fea_multi_case.ri; same \
         retype, and TractionLoad reaches no solver at all today (INV-SF-3)",
    ),
    (
        "tests/prd-gate/fixtures/dcr_material_dimension_silent.ri",
        "youngs_modulus",
        "#6941",
        "leaf β, boundary row B4: `youngs_modulus: 200mm` is read as 0.2 Pa by \
         material_field_si, measured 1e12x wrong at exit 0 with zero Error diagnostics",
    ),
    (
        "tests/prd-gate/fixtures/dcr_reader_ctor_dimension_silent.ri",
        "ex",
        "#6922",
        "leaf η: `FDMCouponOverride(ex: 2mm)` stores 0.002 m and the dimension-blind \
         opt_f64 reads it as 0.002 Pa",
    ),
    (
        "tests/prd-gate/fixtures/dcr_reader_ctor_dimension_silent.ri",
        "line_width",
        "#6922",
        "leaf η: `AsPrintedOptions(line_width: 0.4)` is read as 0.4 METRES by \
         field_scalar — a 1000x error on a 0.4mm extrusion",
    ),
    (
        "tests/prd-gate/fixtures/dcr_reader_ctor_dimension_silent.ri",
        "mass",
        "#6922",
        "leaf ε: `MassProperties(mass: 2m)` is read as 2.0 kg by the blind cell_f64 \
         copy, while the dimension-checking cell_mass_f64 sits unused ~300 lines away",
    ),
    (
        "tests/prd-gate/fixtures/dcr_reader_ctor_dimension_silent.ri",
        "target_frequency",
        "#6941",
        "leaf ζ: `ZVShaper(target_frequency: 50rad/s)` is stored verbatim as rad·s^-1 \
         and the Hz->rad/s marshalling then multiplies by 2π — a 6.28x error",
    ),
    (
        "tests/prd-gate/fixtures/dcr_shaper_frequency_dimension_silent.ri",
        "target_frequency",
        "#6941",
        "leaf ζ signal fixture: the same 6.28x error, but CONSUMED via input_shape so \
         read_scalar_si actually runs — the ctor alone never reaches the reader",
    ),
    (
        "tests/prd-gate/fixtures/dcr_solver_load_dropped_dimensioned.ri",
        "force",
        "#6922",
        "leaf γ1 headline inversion: the units-CORRECT `force: 1000N` contributes \
         EXACTLY ZERO force to solve_elastic_static (max_von_mises 0, iterations 0) \
         where the bare control contributes 1000 N",
    ),
    (
        "tests/prd-gate/fixtures/dcr_yield_stress_dimension_silent.ri",
        "yield_stress",
        "#6941",
        "leaf β, boundary row B5: `yield_stress: 310mm` is stored as Some(0.31 m) and \
         material_field_si reads it as 0.31 Pa, at exit 0 with zero diagnostics",
    ),
];

/// The repo-relative prefix of the `examples/` corpus.
///
/// [`CTOR_CONFORMANCE_MIGRATION_DEBT`](super::examples_smoke::CTOR_CONFORMANCE_MIGRATION_DEBT)
/// is keyed relative to that directory; every key in THIS module is
/// repo-relative. This const is the whole of the difference.
const EXAMPLES_PREFIX: &str = "examples/";

/// Whether a [`CTOR_CONFORMANCE_MIGRATION_DEBT`](super::examples_smoke::CTOR_CONFORMANCE_MIGRATION_DEBT)
/// entry describes the REPO-RELATIVE site `(file, param)`.
///
/// The single place the two tables' key forms are bridged. The debt list is
/// `examples/`-keyed by construction — its own doc forbids the repo-relative
/// spelling, and the gate that consumes it walks `EXAMPLES_DIR` only — so
/// neither table can change shape and the join has to happen here. A file
/// outside `examples/` can never match a debt entry, which is exactly why
/// [`CTOR_CONFORMANCE_CORPUS_RESIDUAL`] has to exist as a sibling table.
///
/// The `(file, param)` matching RULE is not restated here; it is
/// `examples_smoke`'s `debt_entry_matches`, called through.
fn debt_entry_describes(entry: &(&str, &str, &str), file: &str, param: Option<&str>) -> bool {
    file.strip_prefix(EXAMPLES_PREFIX)
        .is_some_and(|key| super::examples_smoke::debt_entry_matches(entry, key, param))
}

/// The reason every
/// [`CTOR_CONFORMANCE_MIGRATION_DEBT`](super::examples_smoke::CTOR_CONFORMANCE_MIGRATION_DEBT)
/// site is deferred.
///
/// That table carries no `why` column — it predates this one, and every entry in
/// it shares one reason — so the reason belongs to the TABLE, not to a row.
/// Stated here once rather than copied into each rendered row, and deliberately
/// not added as a fourth column there: #5847 and #5306 are both chartered
/// against that list by name.
const MIGRATION_DEBT_WHY: &str = "un-migrated examples/ call site that cannot be dimensioned \
     in isolation; waived per-site in CTOR_CONFORMANCE_MIGRATION_DEBT and retired by its \
     owning task's own diff";

/// The `Debug` rendering of `reify_core::Severity::Warning`, which is how
/// [`SurveySite::severity`] carries it.
///
/// Read in exactly ONE place — [`disposition_of`] — so γ's Warning-severity scope
/// is stated once and every consumer inherits it through the resolver instead of
/// re-filtering on severity itself.
const WARNING_SEVERITY: &str = "Warning";

/// γ's per-site ruling on a surveyed site, resolved from the site's measured
/// severity and the waiver tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disposition {
    /// A live task owns retiring the site. `why` says what breaks if someone
    /// migrates it here instead.
    Deferred {
        owning_task: &'static str,
        why: &'static str,
    },
    /// The site carries a ctor-conformance CODE, but not at Warning severity: it
    /// is outside γ's signal, and nobody owns retiring it.
    ///
    /// Its own variant rather than folded into [`Disposition::Unattributed`],
    /// because the two call for OPPOSITE actions — an unattributed row is work,
    /// this is work that does not exist. Folding them told the artifact's reader
    /// to go fix three deliberate rejection fixtures whose violation IS their
    /// content.
    NotApplicable,
    /// A Warning that no table names: it is actionable, and nobody has claimed it.
    Unattributed,
}

impl Disposition {
    /// The artifact cell for this disposition.
    ///
    /// A deferred cell names the owner and the reason so it reads standalone —
    /// the artifact is consumed one row at a time, and a bare cite would send
    /// the reader hunting for a table to find out why. The `n/a` cell instead
    /// stays short and the artifact header explains that class ONCE: it applies
    /// to whole rows identically, so repeating a paragraph per row would be the
    /// copy that rots.
    fn label(self) -> String {
        match self {
            Disposition::Deferred { owning_task, why } => {
                format!("deferred — owned by {owning_task}: {why}")
            }
            Disposition::NotApplicable => {
                "n/a — Error severity, outside the ctor-conformance warning signal: \
                 nothing to retire"
                    .to_owned()
            }
            Disposition::Unattributed => "unattributed — actionable".to_owned(),
        }
    }
}

/// Resolve `site`'s disposition from [`CTOR_CONFORMANCE_CORPUS_RESIDUAL`] and
/// [`CTOR_CONFORMANCE_MIGRATION_DEBT`](super::examples_smoke::CTOR_CONFORMANCE_MIGRATION_DEBT).
///
/// The ONLY place the two tables are unioned. The tables are the single source
/// of truth and the artifact is a projection of them, so nothing else re-derives
/// this mapping — including the corpus-wide check in
/// [`generate_ctor_conformance_corpus_survey`], which calls straight through.
///
/// Consultation order is immaterial:
/// [`ctor_conformance_corpus_residual_is_disjoint_from_migration_debt`] proves
/// no site can be described by both.
///
/// A WARNING whose param could not be recovered is [`Disposition::Unattributed`].
/// Both tables key on `(file, param)`, so there is nothing to match on, and the
/// conservative default is the one that does not invent an owner.
///
/// # Severity decides SCOPE, before any table is consulted
///
/// γ's signal is "zero unwaived ctor-conformance WARNINGS", so a site at any
/// other severity is [`Disposition::NotApplicable`]. The corpus's Error-severity
/// ctor-conformance-CODED sites — `bt1_wrong_kind_union.ri`,
/// `bt6_kind_typed_param.ri`, `raw_lambda_material_field_rejected.ri` — are
/// deliberate rejection fixtures reached from NON-ctor paths (selector
/// composition, overload resolution, trait conformance). They work exactly as
/// intended; giving them an owner would invent work, and leaving them
/// `Unattributed` told the artifact's reader to go delete three other PRDs'
/// signals.
///
/// Stating that scope HERE rather than as a severity filter at each consumer is
/// what keeps the artifact's `disposition` column and
/// [`assert_no_unwaived_ctor_conformance_warnings`] unable to disagree about
/// which sites the signal even covers.
///
/// When δ (#5306) flips `CTOR_FIELD_CONFORMANCE_SEVERITY` to `Error`, this is the
/// line that moves with it. Until it does, every waiver entry reads as STALE and
/// the assertion goes RED naming them — loudly re-scoped, never vacuously green.
fn disposition_of(site: &SurveySite) -> Disposition {
    if site.severity != WARNING_SEVERITY {
        return Disposition::NotApplicable;
    }

    let Some(param) = site.field.as_deref() else {
        return Disposition::Unattributed;
    };

    if let Some(&(_, _, owning_task, why)) = CTOR_CONFORMANCE_CORPUS_RESIDUAL
        .iter()
        .find(|entry| entry.0 == site.file && entry.1 == param)
    {
        return Disposition::Deferred { owning_task, why };
    }

    if let Some(&(_, _, owning_task)) = super::examples_smoke::CTOR_CONFORMANCE_MIGRATION_DEBT
        .iter()
        .find(|entry| debt_entry_describes(entry, &site.file, Some(param)))
    {
        return Disposition::Deferred {
            owning_task,
            why: MIGRATION_DEBT_WHY,
        };
    }

    Disposition::Unattributed
}

/// Panic unless every WARNING-severity ctor-conformance site in `run` is
/// accounted for by exactly one waiver-table entry, and every entry accounts for
/// at least one site.
///
/// This is γ's actual signal — "zero UNWAIVED ctor-conformance warnings" — made
/// repeatable instead of asserted once in a commit message.
///
/// Both directions are reported together, because they are DIFFERENT defects:
///
/// * a site named by NEITHER table is an UNEXPLAINED warning. Someone added an
///   un-migrated call site, or reverted a migration; γ's invariant is broken.
/// * an entry matching NO site is STALE. Its owning task landed and the entry
///   must be deleted in that same diff — exactly the rot
///   `ctor_conformance_migration_debt_entries_are_all_live` catches for the debt
///   list, extended to the whole tracked corpus.
///
/// # Scoped to Warning severity, but it does not say so itself
///
/// The corpus also carries ERROR-severity ctor-conformance-CODED sites —
/// `bt1_wrong_kind_union.ri`, `bt6_kind_typed_param.ri`,
/// `raw_lambda_material_field_rejected.ri`. Those are deliberate REJECTION
/// fixtures reached from non-ctor paths (selector composition, overload
/// resolution, trait conformance): they are working exactly as intended, they
/// are not ctor-conformance warnings, and enumerating them as residual would
/// claim an owner for something nobody needs to retire.
///
/// That scope is [`disposition_of`]'s, not this function's: nothing here reads
/// [`WARNING_SEVERITY`]. Both directions below are decided entirely by the
/// resolver — `Unattributed` is the unexplained set, `Deferred` is the waived set
/// — so the artifact's `disposition` column and this assertion cannot disagree
/// about whether a site is in scope OR about whether it is waived. A severity
/// filter here as well would be a second, silently divergent copy of the scope
/// statement.
fn assert_no_unwaived_ctor_conformance_warnings(run: &SurveyRun) {
    let unexplained: Vec<String> = run
        .sites
        .iter()
        .filter(|s| disposition_of(s) == Disposition::Unattributed)
        .map(|s| {
            format!(
                "  {}:{} :: param '{}'  {}",
                s.file,
                s.line,
                s.field.as_deref().unwrap_or("—"),
                s.message,
            )
        })
        .collect();

    // The waived set, by the same resolver that renders the artifact column.
    // Severity scope rides along rather than being re-stated: only a Warning can
    // resolve to `Deferred`, so an entry whose only site stopped being a warning
    // reads as stale — the loud, correct outcome.
    let waived: Vec<&SurveySite> = run
        .sites
        .iter()
        .filter(|s| matches!(disposition_of(s), Disposition::Deferred { .. }))
        .collect();

    let stale_residual = CTOR_CONFORMANCE_CORPUS_RESIDUAL.iter().filter_map(|entry| {
        let matched = waived
            .iter()
            .any(|s| s.file == entry.0 && s.field.as_deref() == Some(entry.1));
        (!matched).then(|| {
            format!(
                "  {} :: param '{}'  (owner {}, CTOR_CONFORMANCE_CORPUS_RESIDUAL)",
                entry.0, entry.1, entry.2,
            )
        })
    });
    let stale_debt = super::examples_smoke::CTOR_CONFORMANCE_MIGRATION_DEBT
        .iter()
        .filter_map(|entry| {
            let matched = waived
                .iter()
                .any(|s| debt_entry_describes(entry, &s.file, s.field.as_deref()));
            (!matched).then(|| {
                format!(
                    "  {}{} :: param '{}'  (owner {}, CTOR_CONFORMANCE_MIGRATION_DEBT)",
                    EXAMPLES_PREFIX, entry.0, entry.1, entry.2,
                )
            })
        });
    let stale: Vec<String> = stale_residual.chain(stale_debt).collect();

    assert!(
        unexplained.is_empty() && stale.is_empty(),
        "the tracked corpus and the waiver tables disagree: {} unexplained \
         warning(s), {} stale entry/entries.\n\n\
         UNEXPLAINED — a Warning-severity ctor-conformance site named by NEITHER \
         CTOR_CONFORMANCE_CORPUS_RESIDUAL nor CTOR_CONFORMANCE_MIGRATION_DEBT:\n{}\n\n\
         Fix the site. Add a waiver ONLY if a LIVE task genuinely owns retiring it, \
         and then name that task and say what breaks if it is migrated here instead.\n\n\
         STALE — a waiver entry matching no live site:\n{}\n\n\
         The expected case is that the owning task landed: DELETE the entry, in the \
         same diff that retired the site. TWO other causes make EVERY entry go stale \
         at once, and neither is fixed by deleting them: param extraction stopped \
         matching the emitter's `argument '<name>'` wording (fix the extraction), or \
         `CTOR_FIELD_CONFORMANCE_SEVERITY` was flipped to Error by δ/#5306, which puts \
         every site outside this Warning-scoped signal (re-scope `disposition_of`, \
         which is the ONE place that scope is stated).\n\n\
         Scope comes from `disposition_of`: the corpus's three Error-severity \
         ctor-conformance-coded sites are deliberate rejection fixtures reached from \
         non-ctor paths, so they resolve to `n/a` and are neither waived nor counted \
         here.",
        unexplained.len(),
        stale.len(),
        if unexplained.is_empty() {
            "  (none)".to_owned()
        } else {
            unexplained.join("\n")
        },
        if stale.is_empty() {
            "  (none)".to_owned()
        } else {
            stale.join("\n")
        },
    );
}

/// True when `cite` is the repo's canonical `#NNNN` task-cite form.
///
/// Greek-letter aliases (`task ε`), PRD-relative indices (`task-5`) and prose
/// forms (`task 6941`) all resolve to `malformed-cite` under the repo's
/// TODO-citation convention, and a malformed cite is liveness-checkable by
/// nothing — which is the whole value of naming an owner.
fn is_canonical_task_cite(cite: &str) -> bool {
    cite.strip_prefix('#')
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// Every [`CTOR_CONFORMANCE_CORPUS_RESIDUAL`] entry names a file that exists and
/// is a `.ri`.
///
/// Mirrors `ctor_conformance_migration_debt_entries_exist_under_examples_dir`:
/// cheap, and it separates a MIS-TYPED path from an ALREADY-RETIRED site. Both
/// would otherwise surface only as the corpus-wide staleness failure inside the
/// `#[ignore]`d generator, which reads as "already retired" and invites deleting
/// an entry that is still load-bearing.
#[test]
fn ctor_conformance_corpus_residual_entries_name_existing_ri_files() {
    for (path, param, owner, _why) in CTOR_CONFORMANCE_CORPUS_RESIDUAL {
        assert!(
            path.ends_with(".ri"),
            "CTOR_CONFORMANCE_CORPUS_RESIDUAL entry '{path}' (param '{param}', owner \
             {owner}) is not a `.ri` path"
        );
        let full = std::path::Path::new(WORKSPACE_ROOT).join(path);
        assert!(
            full.exists(),
            "CTOR_CONFORMANCE_CORPUS_RESIDUAL entry '{path}' (param '{param}', owner \
             {owner}) does not exist under {WORKSPACE_ROOT}"
        );
    }
}

/// Every [`CTOR_CONFORMANCE_CORPUS_RESIDUAL`] entry names its owner in the
/// canonical `#NNNN` cite form.
///
/// A deferral without a liveness-checkable owner is a permanent hole dressed up
/// as a temporary one — the failure mode `CTOR_CONFORMANCE_GATE_REMEDY`'s remedy
/// 3 forbids by name ("never without an owner").
#[test]
fn ctor_conformance_corpus_residual_entries_cite_a_canonical_task() {
    let malformed: Vec<String> = CTOR_CONFORMANCE_CORPUS_RESIDUAL
        .iter()
        .filter(|(_, _, owner, _)| !is_canonical_task_cite(owner))
        .map(|(path, param, owner, _)| format!("  {path} :: param '{param}'  (owner {owner})"))
        .collect();

    assert!(
        malformed.is_empty(),
        "CTOR_CONFORMANCE_CORPUS_RESIDUAL has {} entry/entries whose owner is not a \
         canonical `#NNNN` task cite:\n{}\n\n\
         A Greek-letter leaf label, a PRD-relative index or a `task NNNN` prose form is \
         a malformed cite: nothing can liveness-check it. Put the leaf label in the \
         `why` column and the task id in the owner column.",
        malformed.len(),
        malformed.join("\n"),
    );
}

/// [`CTOR_CONFORMANCE_CORPUS_RESIDUAL`] is sorted and duplicate-free on
/// `(path, param)`.
///
/// Sorted so a reader can find a site and a diff shows one line per change;
/// duplicate-free because the disposition resolver takes the FIRST match, so a
/// second entry for the same site would be silently unreachable — including one
/// naming a different owner.
#[test]
fn ctor_conformance_corpus_residual_is_sorted_and_duplicate_free() {
    let keys: Vec<(&str, &str)> = CTOR_CONFORMANCE_CORPUS_RESIDUAL
        .iter()
        .map(|(path, param, _, _)| (*path, *param))
        .collect();

    let mut sorted = keys.clone();
    sorted.sort_unstable();
    assert_eq!(
        keys, sorted,
        "CTOR_CONFORMANCE_CORPUS_RESIDUAL must be sorted on (path, param)"
    );

    let mut deduped = sorted.clone();
    deduped.dedup();
    assert_eq!(
        sorted, deduped,
        "CTOR_CONFORMANCE_CORPUS_RESIDUAL must carry at most one entry per \
         (path, param); a second entry for the same site is unreachable"
    );
}

/// [`CTOR_CONFORMANCE_CORPUS_RESIDUAL`] and
/// [`CTOR_CONFORMANCE_MIGRATION_DEBT`](super::examples_smoke::CTOR_CONFORMANCE_MIGRATION_DEBT)
/// describe DISJOINT sites.
///
/// The two tables are siblings, not a merge: one site described in both would
/// drift, and the resolver would have to pick a winner between two owners.
/// Compared after normalising the two key forms through
/// [`debt_entry_describes`], never by eyeballing the spellings.
#[test]
fn ctor_conformance_corpus_residual_is_disjoint_from_migration_debt() {
    let overlap: Vec<String> = CTOR_CONFORMANCE_CORPUS_RESIDUAL
        .iter()
        .filter(|(path, param, _, _)| {
            super::examples_smoke::CTOR_CONFORMANCE_MIGRATION_DEBT
                .iter()
                .any(|entry| debt_entry_describes(entry, path, Some(param)))
        })
        .map(|(path, param, owner, _)| format!("  {path} :: param '{param}'  (owner {owner})"))
        .collect();

    assert!(
        overlap.is_empty(),
        "these site(s) are described by BOTH CTOR_CONFORMANCE_CORPUS_RESIDUAL and \
         CTOR_CONFORMANCE_MIGRATION_DEBT:\n{}\n\n\
         Pick one. CTOR_CONFORMANCE_MIGRATION_DEBT owns sites under examples/, because \
         the gate that consumes it walks EXAMPLES_DIR only and its keys are relative to \
         that directory. CTOR_CONFORMANCE_CORPUS_RESIDUAL owns everything else.",
        overlap.join("\n"),
    );
}

/// Every [`CTOR_CONFORMANCE_CORPUS_RESIDUAL`] entry says WHY it is deferred.
///
/// The owner cite says who retires the site; the `why` says what would break if
/// someone "fixed" it instead. These fixtures are measured before-images whose
/// violation IS their content, so a reader who meets one without that sentence
/// has every reason to migrate it and delete another PRD's signal.
#[test]
fn ctor_conformance_corpus_residual_entries_say_why() {
    for (path, param, owner, why) in CTOR_CONFORMANCE_CORPUS_RESIDUAL {
        assert!(
            !why.trim().is_empty(),
            "CTOR_CONFORMANCE_CORPUS_RESIDUAL entry '{path}' :: param '{param}' (owner \
             {owner}) carries no reason; an unexplained deferral reads as an oversight"
        );
    }
}

/// Expiry guard, GATE-RESIDENT: every [`CTOR_CONFORMANCE_CORPUS_RESIDUAL`] entry
/// must still name a live site that [`disposition_of`] defers.
///
/// The `examples/`-keyed sibling has had this at gate cadence since α
/// (`ctor_conformance_migration_debt_entries_are_all_live`). Without the same
/// guard here, this table's only staleness check lives inside the `#[ignore]`d
/// [`generate_ctor_conformance_corpus_survey`]: when #6941 lands and retires its
/// six sites, the entries would rot until someone remembered to run an ignored
/// test — and a waiver that outlives its site is a permanent hole in the gate at a
/// `(file, param)` pair nobody is looking at any more. The four sibling hygiene
/// tests cannot see it: path existence, cite form, sort order and disjointness are
/// all satisfied by an entry whose site is gone.
///
/// # Cost
///
/// The entries name SEVEN distinct files, so this compiles seven `.ri` members
/// plus the cached stdlib prelude — the same order as
/// [`pinned_clean_files_emit_no_ctor_conformance_diagnostic`], and nowhere near
/// the 689-member corpus walk that keeps the generator `#[ignore]`d per
/// `docs/prds/merge-gate-compile-cost.md`. Deduplication assumes the table's
/// sortedness (its own test) only as an OPTIMISATION: an unsorted table compiles a
/// file twice, which costs time and cannot produce a false pass.
///
/// # `partial` is tolerated here, unlike the clean pin
///
/// These files are RED before-images, and `curvature_rad_literal.ri` carries
/// Error-severity diagnostics TODAY (its own header says so) — a partially
/// compiled member still contributes its ctor sites. No vacuity follows: this
/// assertion needs the sites to be PRESENT, so a file that stops contributing them
/// turns its entries stale and reds this test BY NAME. `not_surveyed` is still
/// asserted, not because it could hide a pass, but because "this fixture no longer
/// parses" and "this site was retired" want opposite fixes.
#[test]
fn ctor_conformance_corpus_residual_entries_are_all_live() {
    let mut corpus: Vec<String> = CTOR_CONFORMANCE_CORPUS_RESIDUAL
        .iter()
        .map(|(path, _, _, _)| (*path).to_owned())
        .collect();
    corpus.dedup();
    let run = survey_corpus(std::path::Path::new(WORKSPACE_ROOT), &corpus);

    assert!(
        run.not_surveyed.is_empty(),
        "every file named by CTOR_CONFORMANCE_CORPUS_RESIDUAL must reach the compile \
         phase, else its entries cannot be checked at all; unreachable: {:?}\n\n\
         These are committed before-images for other PRDs: a read or parse failure here \
         means the fixture was moved, renamed or broken, NOT that its site was retired.",
        run.not_surveyed,
    );

    let stale: Vec<String> = CTOR_CONFORMANCE_CORPUS_RESIDUAL
        .iter()
        .filter(|entry| {
            !run.sites.iter().any(|s| {
                s.file == entry.0
                    && s.field.as_deref() == Some(entry.1)
                    && matches!(disposition_of(s), Disposition::Deferred { .. })
            })
        })
        .map(|(path, param, owner, _)| format!("  {path} :: param '{param}'  (owner {owner})"))
        .collect();

    assert!(
        stale.is_empty(),
        "CTOR_CONFORMANCE_CORPUS_RESIDUAL has {} stale entry/entries — each defers no \
         live site:\n{}\n\n\
         The expected case is that the owning task landed and retired the site: DELETE \
         the entry, in that same diff. Two other causes stale MANY entries at once and \
         are fixed by neither deleting them nor touching the fixtures: param extraction \
         stopped matching the emitter's `argument '<name>'` wording (fix the \
         extraction), or `CTOR_FIELD_CONFORMANCE_SEVERITY` was flipped to Error by \
         δ/#5306, which puts every site outside this Warning-scoped signal (re-scope \
         `disposition_of`, the one place that scope is stated).",
        stale.len(),
        stale.join("\n"),
    );
}

// ─── step 11/12: markdown rendering ─────────────────────────────────────────

/// The EXACT command that regenerates the artifact, committed inside it.
///
/// The `env` prefix is not decoration: reify's PreToolUse hook rewrites bare
/// `cargo test` invocations into condensed `PASS: N | FAIL: M` output. That is
/// harmless here — the generator WRITES the file rather than being scraped from
/// stdout — but it will confuse a reader of the run log who expects to see the
/// usual per-test lines, so the bypass is baked into the published command.
const REGEN_COMMAND: &str = "env cargo test -p reify-compiler --test harness_compilation_surface \
     -- --ignored --exact \
     ctor_conformance_corpus_survey::generate_ctor_conformance_corpus_survey";

/// Render `text` safe for a markdown table cell.
///
/// Both `|` and newlines are neutralised: either one inside a diagnostic
/// message would silently split or truncate the row, and a survey whose table
/// breaks on its most interesting entries is worse than no survey.
fn cell(text: &str) -> String {
    text.replace('|', "\\|")
        .replace(['\n', '\r'], " ")
        .trim()
        .to_owned()
}

/// Render an optional column: `—` when unrecoverable, never empty, never a guess.
fn opt_cell(value: Option<&String>) -> String {
    match value {
        Some(v) => cell(v),
        None => "—".to_owned(),
    }
}

/// The header key that introduces the drift disclosure.
///
/// One spelling, so the renderer and the tests asserting on its presence — and
/// on its ABSENCE, which is the stronger claim — cannot disagree about what a
/// disclosure looks like.
const DRIFT_DISCLOSURE_KEY: &str = "**Drifted `.ri` since the anchor:**";

/// Render the survey artifact.
///
/// Follows the house convention for a generated markdown artifact set by
/// `docs/architecture-audit/g-tool-baseline-report.md`: a
/// `**Captured:** / **Tool:** / **Design:**` header block above a
/// `## How to regenerate` section holding the literal command.
///
/// Deliberate divergence from that report: it pairs with an `#[ignore]`d
/// tolerance-based freshness test because it is a STANDING baseline. This
/// survey is a point-in-time SNAPSHOT that γ will legitimately invalidate — a
/// freshness gate would go red on every γ commit and would be driven to an
/// EMPTY artifact the moment γ reaches its stated signal, destroying the very
/// census that sized it. So the base commit SHA is stamped instead.
///
/// That stamp is the merge base, never the branch tip ([`survey_stamp`]). When
/// tracked `.ri` have drifted from it, they are NAMED in the header rather than
/// refused, so the snapshot stays honest by disclosure ([`SurveyStamp`]).
fn render_survey(run: &SurveyRun, stamp: &SurveyStamp) -> String {
    use std::fmt::Write as _;

    let mut md = String::new();
    let site_count = run.sites.len();

    // ── header ──────────────────────────────────────────────────────────────
    md.push_str("# Struct-ctor field-type conformance — corpus survey\n\n");
    let _ = writeln!(md, "**Base commit:** `{}`", stamp.anchor);
    let _ = writeln!(
        md,
        "**Tool:** `crates/reify-compiler/tests/harness_compilation_surface/ctor_conformance_corpus_survey.rs`"
    );
    md.push_str("**Design:** `docs/prds/struct-ctor-field-type-conformance.md` (task β, §8)\n");
    let _ = writeln!(md, "**Sites:** {site_count}");
    let _ = writeln!(
        md,
        "**Corpus:** {} tracked `.ri`; {} surveyed, {} not surveyed, {} partial",
        run.total,
        run.surveyed,
        run.not_surveyed.len(),
        run.partial.len()
    );

    // Rendered ONLY when something drifted: an undrifted run must carry no
    // disclosure at all, so the artifact grows no permanent "0 files drifted"
    // row and two undrifted runs stay byte-comparable.
    if !stamp.drifted_ri.is_empty() {
        let _ = write!(
            md,
            "\n\
            {DRIFT_DISCLOSURE_KEY} {n} tracked `.ri` differ between the anchor and the\n\
            commit surveyed, so for those files the anchor names OLDER bytes than the rows\n\
            below describe. They are disclosed rather than refused because they are\n\
            COMMITTED: each is reachable from the surveyed commit, so a reader can read back\n\
            exactly what was swept. (Uncommitted bytes are reachable from no commit, which\n\
            is why a dirty tree is refused outright instead — see `stamp_decision`.)\n\
            \n",
            n = stamp.drifted_ri.len(),
        );
        for path in &stamp.drifted_ri {
            let _ = writeln!(md, "- `{}`", cell(path));
        }
    }

    md.push_str(
        "\n\
        This is a point-in-time **snapshot**, not a freshness-gated golden file. γ will\n\
        legitimately invalidate it — that is the point. Its job is to enumerate and size,\n\
        once, at the base commit stamped above.\n\
        \n\
        ## Provenance\n\
        \n\
        Every row and every count here is **machine-generated — zero hand-derived\n\
        entries**. The corpus is `git ls-files -- '*.ri'`; each member is compiled with\n\
        the α(+ε) warn-stage compiler in-process (`parse_with_stdlib` →\n\
        `compile_with_stdlib`) and every diagnostic carrying one of the seven\n\
        ctor-conformance codes becomes one row. The `file:line` comes from the\n\
        diagnostic's own label span; `expected`/`found` come from the label message.\n\
        Nothing below was typed in by hand, and a column that could not be recovered\n\
        renders as `—` rather than as a guess.\n\
        \n\
        Three things to know before reading a row:\n\
        \n\
        - **`line` is the CTOR CALL-SITE line, not the offending argument's line.** α\n\
          anchors the label at the `Foo(...)` call's own span (PRD §10 Q1;\n\
          `compile_builder/entities_phase.rs`), so a multi-line ctor reports the line of\n\
          its opening `Foo(`. The offending argument is named in the `field` column and\n\
          sits within that call — e.g.\n\
          `examples/trajectory/printer_print_envelope.ri:169` is the `TOTSShaper(` line,\n\
          while `velocity_limit: 300.0` is three lines further down.\n\
        - **`def` is whatever identifier sits at that anchor, and `def source` says where\n\
          it came from.** The recovery reads an identifier followed by `(` — which cannot\n\
          by itself tell a `structure def` ctor from a plain function call. A few rows\n\
          carry codes that reach this survey from a NON-ctor path (selector composition,\n\
          overload resolution), where that identifier is a *function* name. Those are not\n\
          left in the actionable group: a recovered name is cross-checked against every\n\
          `structure def` declared in the corpus and the stdlib, and a name that does not\n\
          resolve is filed under *name recovered, but it is not a known structure def*.\n\
        - **A `—` in `def` is explained, not asserted.** The `def source` column carries\n\
          the machine-derived reason recovery failed for that specific row (span starts at\n\
          a non-identifier, identifier not followed by `(`, span out of range, …), so no\n\
          prose here has to guess a cause on a reader's behalf.\n\
        \n\
        The **`disposition` column is γ's RULING**, projected from the site's measured\n\
        severity and the two per-site waiver tables (`CTOR_CONFORMANCE_CORPUS_RESIDUAL`\n\
        in the generator, `CTOR_CONFORMANCE_MIGRATION_DEBT` in the sibling\n\
        `examples_smoke.rs`) rather than typed here. It has three states, and they call\n\
        for three DIFFERENT actions:\n\
        \n\
        - **`deferred`** names the LIVE task that owns retiring the site, and the reason\n\
        migrating it here would destroy something — most of these are committed RED\n\
        before-images whose violation IS the fixture's content. Leave them alone.\n\
        - **`n/a`** carries a ctor-conformance CODE but at **Error** severity, which is\n\
        outside the zero-ctor-conformance-warnings signal entirely. Every such row today\n\
        is a deliberate REJECTION fixture reached from a NON-ctor path (selector\n\
        composition, overload resolution, trait conformance): the rejection IS the\n\
        behaviour under test. **Not actionable, and not residual either** — it carries no\n\
        owner because it needs none, and reading it as unclaimed work would send you to\n\
        delete another PRD’s signal.\n\
        - **`unattributed`** is a warning claimed by nobody: that is the actionable\n\
        state, and after γ the corpus holds none.\n\
        \n\
        The **`hint` column is ADVISORY**, derived purely from the (expected, found)\n\
        type pair. It is **not** a D9 ruling. PRD §4 D9 defines the split between class\n\
        (1) *call-site bug* and class (2) *wrong declared field type* as \"per-case\n\
        judgment … whichever is the actual bug\" and assigns it to **γ**. γ has now\n\
        ruled, and the ruling is the `disposition` column beside the hint: where the two\n\
        disagree, the disposition wins. What β decided mechanically is the `owner`\n\
        grouping below.\n\
        \n\
        ## Format (PRD §10 Q6)\n\
        \n\
        **Q6 is answered here — grouped by D9 owner class, with a flat\n\
        `(file, line, field)`-sorted table inside each group.** Grouping first by owner\n\
        makes the FEA do-not-touch partition unmissable for γ, whose actual consumption\n\
        question is *which sites may I touch*; a flat table inside each group keeps the\n\
        result directly sizable and sortable. Recorded in this artifact header rather\n\
        than by editing the PRD's §10, which the sibling α/γ/δ/ζ tasks concurrently read.\n\
        \n",
    );

    // ── site groups, FEA first ──────────────────────────────────────────────
    md.push_str("## Sites\n\n");
    if site_count == 0 {
        md.push_str(
            "**No ctor-conformance sites were found in the surveyed corpus.** This is an\n\
             explicit zero, not a truncated run — see the coverage section below for what\n\
             was and was not surveyed.\n\n",
        );
    }
    for owner in Owner::render_order() {
        let mut group: Vec<&SurveySite> = run.sites.iter().filter(|s| s.owner == owner).collect();
        group.sort_by(|a, b| (&a.file, a.line, &a.field).cmp(&(&b.file, b.line, &b.field)));

        let _ = writeln!(md, "### {} — {} site(s)\n", owner.title(), group.len());
        match owner {
            Owner::FeaDeferredToV06 => md.push_str(
                "Per PRD §4 D9, these defs are declared in the FEA stdlib modules: γ may make\n\
                 **call-site changes ONLY**. Field-type flips remain v0.6-owned\n\
                 (`docs/prds/v0_6/fea-load-support-selector-migration.md`). **DO NOT FIX the\n\
                 declared field types here.**\n\n",
            ),
            Owner::NonFea => md.push_str(
                "The recovered name IS a `structure def` declared in the corpus or the stdlib,\n\
                 and it is not FEA-owned. D9's per-case judgment applied here, and **γ has\n\
                 now ruled every Warning row in this group** — read the `disposition` column.\n\
                 A site γ judged a call-site bug was fixed and is simply absent below; a site γ\n\
                 deferred names the LIVE task that owns retiring it.\n\n\
                 That check is against ONE GLOBAL namespace — *some* corpus or stdlib file\n\
                 declares the name, not necessarily one this row's file can see. See named\n\
                 limitation 3 below before treating a row here as actionable.\n\n",
            ),
            Owner::UnresolvedDef => md.push_str(
                "An identifier was recovered at the diagnostic's anchor, but it is not a\n\
                 `structure def` declared anywhere in the corpus or the stdlib. Recovery reads\n\
                 an identifier followed by `(`, which cannot distinguish a ctor from a plain\n\
                 function call, so these are typically FUNCTION names reaching the survey from\n\
                 a non-ctor path (selector composition, overload resolution) — the `severity`\n\
                 and `message` columns show which. Held out of the actionable group rather than\n\
                 sized into it. **Triage manually before touching.**\n\n",
            ),
            Owner::Unknown => md.push_str(
                "No def name could be attributed. The `def source` column gives the\n\
                 machine-derived reason PER ROW rather than asserting one cause for the group:\n\
                 known shapes that land here include the sub `=` per-arg anchor and param\n\
                 default-initializer checks, both of which anchor the label somewhere other\n\
                 than a `Def(` call site. Deliberately its own group: folding an\n\
                 unattributable site into the touchable pile is the one classification error\n\
                 with a real cost. **Triage manually before touching.**\n\n",
            ),
        }
        if group.is_empty() {
            md.push_str("_(none)_\n\n");
            continue;
        }
        md.push_str(
            "| site | def | def source | field | expected | found | code | severity | hint (advisory) | disposition (γ ruling) | message |\n\
             |---|---|---|---|---|---|---|---|---|---|---|\n",
        );
        for s in group {
            let _ = writeln!(
                md,
                "| `{}:{}` | {} | {} | {} | {} | {} | `{}` | {} | {} | {} | {} |",
                cell(&s.file),
                s.line,
                opt_cell(s.def.as_ref()),
                cell(s.def_origin.label()),
                opt_cell(s.field.as_ref()),
                opt_cell(s.expected.as_ref()),
                opt_cell(s.found.as_ref()),
                cell(&s.code),
                cell(&s.severity),
                cell(&remedy_hint(s.expected.as_deref(), s.found.as_deref())),
                cell(&disposition_of(s).label()),
                cell(&s.message),
            );
        }
        md.push('\n');
    }

    // ── coverage + limitations ──────────────────────────────────────────────
    md.push_str("## Coverage and limitations\n\n");
    let _ = writeln!(
        md,
        "Of {} tracked `.ri` members, **{} were surveyed** and **{} were not**. \
         A further **{}** were surveyed only PARTIALLY. Both are listed below rather \
         than dropped: a bounded sweep that does not state what it skipped reads as \
         full coverage and would under-size γ.\n",
        run.total,
        run.surveyed,
        run.not_surveyed.len(),
        run.partial.len()
    );

    md.push_str("### Not surveyed (contributed no sites)\n\n");
    if run.not_surveyed.is_empty() {
        md.push_str("_(none — every tracked member reached the compile phase)_\n\n");
    } else {
        md.push_str("| file | reason |\n|---|---|\n");
        for (file, reason) in &run.not_surveyed {
            let _ = writeln!(md, "| `{}` | `{}` |", cell(file), cell(reason));
        }
        md.push('\n');
    }

    md.push_str(
        "### Partially surveyed (sites collected, but the file also failed to compile)\n\n",
    );
    if run.partial.is_empty() {
        md.push_str("_(none)_\n\n");
    } else {
        md.push_str("| file | reason |\n|---|---|\n");
        for (file, reason) in &run.partial {
            let _ = writeln!(md, "| `{}` | `{}` |", cell(file), cell(reason));
        }
        md.push('\n');
    }

    md.push_str(
        "### Named limitations\n\
        \n\
        1. **Inline Rust-string `.ri` fixtures are not file-enumerable.** The task's\n\
           second half — the Rust test suite's inline fixtures and goldens — lives inside\n\
           `const SOURCE: &str = r#\"…\"#` literals, which `git ls-files` cannot reach and\n\
           which could only be swept by changing the compiler (out of scope for this\n\
           read-only survey). Their coverage is **transitive, and stated as such rather\n\
           than claimed**: the `--scope all --profile both` merge gate is green at the\n\
           base commit above, and the landed α/ε gates\n\
           (`no_example_emits_ctor_field_conformance_diagnostics`, the\n\
           `struct_ctor_field_conformance_tests` suite) already assert on the\n\
           ctor-conformance codes.\n\
        2. **`compile_with_stdlib` is the SINGLE-FILE path.** `reify check` instead uses\n\
           `module_dag::compile_entry_with_stdlib_cfg_checked`, which follows `#cfg`-gated\n\
           user imports and runs `SimpleConstraintChecker`. Multi-module corpus members\n\
           (the `examples/module_visibility/consumer.ri` class) therefore cannot resolve\n\
           standalone and appear above under *not surveyed* or *partially surveyed* with\n\
           their reason, rather than being silently dropped.\n\
        3. **`def` resolution uses ONE GLOBAL namespace, not per-file module scope.** The\n\
           `owner` grouping cross-checks a recovered name against every `structure def`\n\
           declared anywhere in the corpus plus the stdlib — it does NOT ask whether that\n\
           declaration is visible from the file the row sits in. Two consequences, and\n\
           only one of them is safe:\n\
           **(a)** a row lands in *non-FEA — γ per-case judgment* whenever SOME corpus\n\
           file declares that name, even if the row's own file cannot see it. That is\n\
           over-inclusion in the TOUCHABLE direction, so **before treating a `non-FEA` row\n\
           as actionable, confirm the `def` is declared in that row's own file or one of\n\
           its imports.** The `message` and `severity` columns usually settle it in one\n\
           read.\n\
           **(b)** symmetrically, a non-FEA file that happened to declare a name the FEA\n\
           stdlib also declares would pull its sites into the do-not-touch partition. That\n\
           direction over-defers rather than over-touches, so it costs γ sizing accuracy,\n\
           never a wrong edit.\n\
           Per-member scoping (each file's own declarations plus its imports) would remove\n\
           the approximation, at the cost of resolving the import graph for every member —\n\
           more machinery than a one-shot snapshot warrants, so the approximation is stated\n\
           here instead of hidden.\n\
        \n",
    );

    // ── regeneration ────────────────────────────────────────────────────────
    md.push_str("## How to regenerate\n\n```bash\n");
    let _ = writeln!(md, "{REGEN_COMMAND}");
    md.push_str("```\n\n");
    md.push_str(
        "The generator is `#[ignore]`d: it compiles the whole tracked corpus, which is\n\
        ~2.5× the `examples/` walk already documented as the most expensive thing that\n\
        test binary does, and paying that on every merge gate would fight\n\
        `docs/prds/merge-gate-compile-cost.md`. Everything the generator *decides* —\n\
        enumeration, span→line, def/field/type extraction, D9 classification and this\n\
        rendering — is unit-tested on every gate run against synthetic inputs, plus one\n\
        cheap three-file end-to-end sweep, so the pipeline cannot bit-rot between runs.\n\
        \n\
        Set `REIFY_CTOR_SURVEY_OUT` to write elsewhere (e.g. to diff a fresh run against\n\
        the committed copy without dirtying the tree).\n\
        \n\
        > The `env` prefix on the command above bypasses reify's PreToolUse hook, which\n\
        > condenses `cargo test` output. It is harmless here — the generator writes the\n\
        > file rather than being scraped from stdout — but without it a reader of the run\n\
        > log sees only a `PASS: N | FAIL: M` summary and may think the sweep did nothing.\n",
    );

    md
}

/// A `SurveyRun` assembled by hand for the renderer tests, so no corpus
/// compile is needed to exercise the artifact's whole contract.
#[cfg(test)]
fn synth_site(file: &str, line: u32, def: &str, field: &str, owner: Owner) -> SurveySite {
    SurveySite {
        file: file.to_owned(),
        line,
        def: Some(def.to_owned()),
        def_origin: DefOrigin::CallSiteAnchor,
        field: Some(field.to_owned()),
        expected: Some("FaceSelector".to_owned()),
        found: Some("String".to_owned()),
        code: "ArgTypeMismatch".to_owned(),
        severity: "Warning".to_owned(),
        // `FaceSelector`, matching the `expected` cell above: that is the
        // rendering `reify_core::Type::Selector(SelectorKind::Face)` actually
        // Displays. `Selector(Face)` appears nowhere in real compiler output.
        message: format!(
            "argument '{field}' has type 'String' but param '{field}' requires type 'FaceSelector'"
        ),
        owner,
    }
}

#[test]
fn render_survey_states_a_site_count_that_equals_the_rendered_rows() {
    let run = SurveyRun {
        total: 660,
        surveyed: 640,
        not_surveyed: vec![("x.ri".to_owned(), "parse-error".to_owned())],
        partial: vec![("y.ri".to_owned(), "compile-error".to_owned())],
        sites: vec![
            synth_site("a.ri", 3, "PointLoad", "point", Owner::FeaDeferredToV06),
            synth_site("b.ri", 7, "Widget", "label", Owner::NonFea),
        ],
    };
    let md = render_survey(&run, &SurveyStamp::at("deadbeef"));

    // The stated count is COMPUTED, never typed — that is the task's
    // "site count stated" signal, and it must equal the rows actually drawn.
    let rows = rendered_site_rows(&md);
    assert_eq!(rows, 2, "two sites must draw two table rows, got:\n{md}");
    assert!(
        md.contains("**Sites:** 2"),
        "the header must state the site count; got:\n{md}"
    );
    assert!(md.contains("deadbeef"), "the base commit must be stamped");
    assert!(md.contains("660"), "the live corpus total must be stated");
    assert!(
        md.contains("640"),
        "the surveyed count must be stated so the denominator is visible"
    );

    // …and the same identity must hold for a run carrying one site of EVERY
    // owner class, which is the case the two-site run above cannot see. If the
    // renderer ever stops emitting a group, the stated `**Sites:** N` keeps
    // counting those sites while the table stops drawing them — a class of sites
    // silently vanishing from the artifact behind an unchanged count. Built from
    // `Owner::render_order()` so a newly-added variant is covered the moment it
    // is declared, with no edit here.
    let every_class: Vec<SurveySite> = Owner::render_order()
        .into_iter()
        .enumerate()
        .map(|(i, owner)| synth_site(&format!("f{i}.ri"), 1, "W", "field", owner))
        .collect();
    let n = every_class.len();
    let run = SurveyRun {
        total: n,
        surveyed: n,
        not_surveyed: vec![],
        partial: vec![],
        sites: every_class,
    };
    let md = render_survey(&run, &SurveyStamp::at("deadbeef"));
    assert_eq!(
        rendered_site_rows(&md),
        run.sites.len(),
        "every site handed in must be DRAWN, not merely counted — one owner class \
         per site, {n} sites:\n{md}"
    );
    assert!(
        md.contains(&format!("**Sites:** {n}")),
        "the stated count must equal the rows drawn; got:\n{md}"
    );
}

/// Count the site rows actually drawn in a rendered artifact.
///
/// Site rows are the only table rows whose first cell is a `` `<file>.ri:<line>` ``
/// anchor, so this cannot pick up the coverage tables.
#[cfg(test)]
fn rendered_site_rows(md: &str) -> usize {
    md.lines()
        .filter(|l| l.starts_with("| `") && l.contains(".ri:"))
        .count()
}

#[test]
fn render_survey_groups_by_d9_owner_with_fea_first_and_marked_do_not_fix() {
    let mut unresolved = synth_site("u.ri", 1, "union", "arg", Owner::UnresolvedDef);
    unresolved.def_origin = DefOrigin::CallSiteAnchor;
    let mut unknown = synth_site("m.ri", 1, "Mystery", "f", Owner::Unknown);
    unknown.def = None;
    unknown.def_origin = DefOrigin::SpanNotIdentifier;

    let run = SurveyRun {
        total: 4,
        surveyed: 4,
        not_surveyed: vec![],
        partial: vec![],
        sites: vec![
            synth_site("z.ri", 1, "Widget", "label", Owner::NonFea),
            synth_site("a.ri", 1, "PointLoad", "point", Owner::FeaDeferredToV06),
            unknown,
            unresolved,
        ],
    };
    let md = render_survey(&run, &SurveyStamp::at("cafe1234"));

    // Anchor every ordering probe on the rendered `### <title>` heading, never on
    // a bare substring: the artifact's own `## Format` prose mentions "FEA"
    // above `## Sites`, so `md.find("FEA")` would return a fixed header offset
    // that precedes EVERY group and make this ordering assertion vacuously
    // green — including when the FEA group is emitted last or dropped entirely.
    let heading_at = |owner: Owner| {
        md.find(&format!("### {}", owner.title()))
            .unwrap_or_else(|| panic!("missing group heading for {owner:?}:\n{md}"))
    };
    let fea_at = heading_at(Owner::FeaDeferredToV06);
    let non_fea_at = heading_at(Owner::NonFea);
    assert!(
        fea_at < non_fea_at,
        "the do-not-touch FEA partition must come FIRST — γ's first question is \
         which sites it may touch:\n{md}"
    );
    assert!(
        md.contains("DO NOT FIX"),
        "the FEA group must be explicitly labelled do-not-fix:\n{md}"
    );
    assert!(
        md.contains("unattributed"),
        "the Unknown bucket must be rendered as its own group, not folded away"
    );

    // EVERY owner class must render its own group. A class the renderer forgets
    // is a class of sites that silently vanishes from the artifact — the
    // survey's one unacceptable failure.
    //
    // The list is `Owner::render_order()`, i.e. DERIVED from the enum via
    // `strum::EnumIter`, never a hand-written copy: a copy here would have to be
    // kept in step with the renderer's own copy, and two literals that must
    // agree is precisely the drift this test claims to catch. Because the run
    // above is also built from that derived list, adding a fifth variant makes
    // this test demand a fifth group rather than quietly ignoring it.
    for owner in Owner::render_order() {
        let heading = format!("### {} — 1 site(s)", owner.title());
        assert!(
            md.contains(&heading),
            "every owner class must render its own group with its own count; missing \
             {heading:?}:\n{md}"
        );
    }

    // The two non-actionable triage buckets must sort AFTER the actionable one,
    // so γ reads its own work first and does not mistake a function-call row for
    // a ctor site it owns.
    let unresolved_at = heading_at(Owner::UnresolvedDef);
    assert!(
        non_fea_at < unresolved_at,
        "the actionable non-FEA group must precede the manual-triage buckets:\n{md}"
    );

    // …and the recovered-but-unknown name is NOT sized into γ's actionable pile.
    let non_fea_block = &md[non_fea_at..unresolved_at];
    assert!(
        !non_fea_block.contains("`u.ri:1`"),
        "`union` is a function name, not a structure def — it must not appear in the \
         actionable non-FEA group:\n{non_fea_block}"
    );
}

#[test]
fn render_survey_orders_rows_deterministically_within_a_group() {
    let ordered = [
        synth_site("a.ri", 2, "W", "alpha", Owner::NonFea),
        synth_site("a.ri", 9, "W", "beta", Owner::NonFea),
        synth_site("b.ri", 1, "W", "gamma", Owner::NonFea),
    ];
    let mut shuffled = vec![ordered[2].clone(), ordered[0].clone(), ordered[1].clone()];
    // A same-(file,line) pair discriminated only by field must still sort.
    shuffled.push(synth_site("a.ri", 2, "W", "aardvark", Owner::NonFea));

    let mk = |sites: Vec<SurveySite>| SurveyRun {
        total: sites.len(),
        surveyed: sites.len(),
        not_surveyed: vec![],
        partial: vec![],
        sites,
    };
    let mut sorted = shuffled.clone();
    sorted.sort_by(|a, b| (&a.file, a.line, &a.field).cmp(&(&b.file, b.line, &b.field)));

    assert_eq!(
        render_survey(&mk(shuffled), &SurveyStamp::at("sha")),
        render_survey(&mk(sorted), &SurveyStamp::at("sha")),
        "rows must render in (file, line, field) order regardless of input order"
    );
}

#[test]
fn render_survey_escapes_pipes_and_newlines_so_a_message_cannot_break_the_table() {
    let mut site = synth_site("a.ri", 1, "W", "f", Owner::NonFea);
    site.message = "a | b\nsecond line | c".to_owned();
    site.field = Some("has|pipe".to_owned());
    let run = SurveyRun {
        total: 1,
        surveyed: 1,
        not_surveyed: vec![],
        partial: vec![],
        sites: vec![site],
    };
    let md = render_survey(&run, &SurveyStamp::at("sha"));

    let row = md
        .lines()
        .find(|l| l.starts_with("| `a.ri:1`"))
        .expect("the site row");
    assert!(
        !row.contains("a | b"),
        "a raw pipe inside a message would split the cell: {row:?}"
    );
    assert!(
        row.contains("second line"),
        "the escaped message must still carry its full text: {row:?}"
    );
    assert!(
        !md.contains("has|pipe"),
        "a pipe inside ANY cell must be escaped, not just the message"
    );
}

#[test]
fn render_survey_writes_an_em_dash_for_every_unrecoverable_cell() {
    let site = SurveySite {
        file: "a.ri".to_owned(),
        line: 1,
        def: None,
        def_origin: DefOrigin::SpanNotIdentifier,
        field: None,
        expected: None,
        found: None,
        code: "CtorArity".to_owned(),
        severity: "Warning".to_owned(),
        message: "E_CTOR_ARITY: Bar() expects at most 1 argument, got 2".to_owned(),
        owner: Owner::Unknown,
    };
    let run = SurveyRun {
        total: 1,
        surveyed: 1,
        not_surveyed: vec![],
        partial: vec![],
        sites: vec![site],
    };
    let md = render_survey(&run, &SurveyStamp::at("sha"));
    let row = md
        .lines()
        .find(|l| l.starts_with("| `a.ri:1`"))
        .expect("the site row");
    // Check the four cells BY POSITION rather than counting em-dashes in the
    // whole row: the neutral hint string legitimately contains one too, so a
    // raw count is an imprecise proxy for what this test actually means.
    let cells: Vec<&str> = row.split('|').map(str::trim).collect();
    for (idx, name) in [(2, "def"), (4, "field"), (5, "expected"), (6, "found")] {
        assert_eq!(
            cells[idx], "—",
            "the {name} cell must render as an em-dash, never an empty or invented \
             cell: {row:?}"
        );
    }
    // …but the `def source` cell is NEVER an em-dash: an unrecovered def has a
    // machine-derived REASON, which is what stops the artifact from asserting a
    // cause for the whole group in prose.
    assert_eq!(
        cells[3],
        DefOrigin::SpanNotIdentifier.label(),
        "an unrecovered def must carry its machine-derived recovery-failure reason, \
         not a second em-dash: {row:?}"
    );
    assert!(
        !row.contains("||"),
        "no cell may be rendered empty: {row:?}"
    );
    assert!(
        cells[11].starts_with("E_CTOR_ARITY:"),
        "the raw message must still be carried verbatim: {row:?}"
    );
}

#[test]
fn render_survey_renders_the_zero_site_case_explicitly() {
    let run = SurveyRun {
        total: 660,
        surveyed: 660,
        not_surveyed: vec![],
        partial: vec![],
        sites: vec![],
    };
    let md = render_survey(&run, &SurveyStamp::at("sha"));
    assert!(
        md.contains("**Sites:** 0"),
        "the count must still be stated"
    );
    assert!(
        md.to_lowercase().contains("no ctor-conformance"),
        "a zero-site outcome must be rendered as an explicit statement, never an \
         empty table a reader could mistake for a truncated run:\n{md}"
    );
}

#[test]
fn render_survey_reports_the_recovery_reason_instead_of_asserting_a_cause() {
    // The whole point of the `def source` column: an earlier Unknown-group blurb
    // asserted "these sites come through the sub `=` per-arg anchor", which was
    // false for BOTH rows actually in the committed artifact (they are param
    // default initializers). The renderer must therefore report per row what the
    // code measured, rather than naming one cause as THE cause for the group.
    //
    // That property is pinned BEHAVIOURALLY below (the row's own
    // `DefOrigin` label and the `def source` column must both reach the
    // artifact), never by a negative pin on the group blurb's wording. An
    // earlier draft carried `!md.contains("these sites come\nthrough the sub
    // `=` per-arg anchor")`, which embedded a newline at a position the
    // renderer never wraps at: it was vacuously true, would have stayed true
    // under any rewording, and pinned nothing. Do not reintroduce it — a
    // wording pin on generated prose can only rot.
    let mut site = synth_site("a.ri", 1, "W", "f", Owner::Unknown);
    site.def = None;
    site.def_origin = DefOrigin::SpanNotIdentifier;
    let run = SurveyRun {
        total: 1,
        surveyed: 1,
        not_surveyed: vec![],
        partial: vec![],
        sites: vec![site],
    };
    let md = render_survey(&run, &SurveyStamp::at("sha"));

    assert!(
        md.contains(DefOrigin::SpanNotIdentifier.label()),
        "the row's machine-derived recovery-failure reason must appear in the \
         artifact:\n{md}"
    );
    assert!(
        md.contains("| def source |"),
        "the `def source` column must be part of the table header:\n{md}"
    );

    // And a RECOVERED def reports where it came from, not a blank.
    let recovered = SurveyRun {
        total: 1,
        surveyed: 1,
        not_surveyed: vec![],
        partial: vec![],
        sites: vec![synth_site("b.ri", 2, "Widget", "label", Owner::NonFea)],
    };
    let md = render_survey(&recovered, &SurveyStamp::at("sha"));
    assert!(
        md.contains(DefOrigin::CallSiteAnchor.label()),
        "a recovered def must still say HOW it was recovered:\n{md}"
    );
}

#[test]
fn render_survey_names_every_drifted_ri_without_disturbing_the_anchor() {
    // The disclosure's whole job: a reader must be able to see WHICH files the
    // anchor no longer describes, by name, without running git. Machine-derived
    // from the same `git diff --name-only` read the header stamps — no path
    // here is hand-typed, the same rule every other row in this artifact lives
    // under.
    let run = one_site_run();
    let drifted = SurveyStamp {
        anchor: "cafe1234".to_owned(),
        drifted_ri: vec![
            "tests/prd-gate/fixtures/one.ri".to_owned(),
            "tree-sitter-reify/test/fixtures/two.ri".to_owned(),
        ],
    };
    let md = render_survey(&run, &drifted);

    assert!(
        md.contains(DRIFT_DISCLOSURE_KEY),
        "a drifted stamp must disclose; got:\n{md}"
    );
    for path in &drifted.drifted_ri {
        assert!(
            md.contains(path.as_str()),
            "the disclosure must name {path} — a path it drops is a path no \
             reader can know about; got:\n{md}"
        );
    }
    assert!(
        md.contains(&format!("{} tracked", drifted.drifted_ri.len())),
        "the stated count must be COMPUTED from the disclosed list, never \
         typed; got:\n{md}"
    );
    // Drift discloses; it never re-anchors. Read through the same parser the
    // gate-resident ancestry guard uses, so this pins the line that guard reads.
    assert_eq!(
        parse_stamped_base_commit(&md),
        Some("cafe1234"),
        "the merge base stays THE stamped anchor — a rewritable branch tip must \
         never be promoted into that line; got:\n{md}"
    );
}

#[test]
fn render_survey_omits_the_disclosure_entirely_when_nothing_drifted() {
    // The undrifted run is the common one, and it must render EXACTLY what it
    // rendered before the disclosure existed: no "0 files drifted" noise row,
    // so two runs generated on `main` stay byte-comparable with each other.
    let run = one_site_run();
    let without = render_survey(&run, &SurveyStamp::at("cafe1234"));
    assert!(
        !without.contains(DRIFT_DISCLOSURE_KEY),
        "an undrifted stamp must render no disclosure at all; got:\n{without}"
    );

    // …and the disclosure is purely ADDITIVE: it is inserted, and changes not
    // one byte above or below itself. Asserted as prefix/suffix identity rather
    // than by eyeballing the two renderings.
    let with = render_survey(
        &run,
        &SurveyStamp {
            anchor: "cafe1234".to_owned(),
            drifted_ri: vec!["tests/prd-gate/fixtures/one.ri".to_owned()],
        },
    );
    let at = with
        .find(DRIFT_DISCLOSURE_KEY)
        .expect("the drifted rendering must carry the disclosure key");
    assert_eq!(
        &with[..at],
        &without[..at],
        "everything ABOVE the disclosure must be byte-identical either way"
    );
    assert!(
        with.ends_with(&without[at..]),
        "everything BELOW the disclosure must be byte-identical either way"
    );
}

/// A one-site run: the smallest thing that renders every section of the
/// artifact, for tests whose subject is the header rather than the rows.
#[cfg(test)]
fn one_site_run() -> SurveyRun {
    SurveyRun {
        total: 1,
        surveyed: 1,
        not_surveyed: vec![],
        partial: vec![],
        sites: vec![synth_site("a.ri", 3, "Widget", "label", Owner::NonFea)],
    }
}

#[test]
fn render_survey_carries_the_regeneration_command_and_the_coverage_section() {
    let run = SurveyRun {
        total: 4,
        surveyed: 2,
        not_surveyed: vec![
            ("bad.ri".to_owned(), "parse-error".to_owned()),
            ("gone.ri".to_owned(), "read-error".to_owned()),
        ],
        partial: vec![("multi.ri".to_owned(), "compile-error".to_owned())],
        sites: vec![synth_site("a.ri", 1, "W", "f", Owner::NonFea)],
    };
    let md = render_survey(&run, &SurveyStamp::at("sha"));

    assert!(
        md.contains("## How to regenerate"),
        "house convention for a generated artifact (cf. \
         docs/architecture-audit/g-tool-baseline-report.md)"
    );
    assert!(
        md.contains(REGEN_COMMAND),
        "the EXACT regeneration command must appear verbatim:\n{md}"
    );

    // Coverage: every not-surveyed and partial member, with its reason.
    for (name, reason) in [
        ("bad.ri", "parse-error"),
        ("gone.ri", "read-error"),
        ("multi.ri", "compile-error"),
    ] {
        assert!(
            md.contains(name) && md.contains(reason),
            "the coverage section must list {name} with reason {reason}:\n{md}"
        );
    }
}

/// The header label of the disposition column, and the one place the tests read
/// it from — so a row's disposition is located BY COLUMN NAME rather than by a
/// hard-coded index that silently shifts when a column is inserted.
#[cfg(test)]
const DISPOSITION_COLUMN: &str = "disposition (γ ruling)";

/// The disposition cell of the site row anchored at `row_anchor`.
#[cfg(test)]
fn disposition_cell(md: &str, row_anchor: &str) -> String {
    let header = md
        .lines()
        .find(|l| l.starts_with("| site |"))
        .unwrap_or_else(|| panic!("the site table header must be rendered:\n{md}"));
    let idx = header
        .split('|')
        .map(str::trim)
        .position(|c| c == DISPOSITION_COLUMN)
        .unwrap_or_else(|| {
            panic!("the site table header must carry a `{DISPOSITION_COLUMN}` column:\n{header}")
        });
    let row = md
        .lines()
        .find(|l| l.starts_with(row_anchor))
        .unwrap_or_else(|| panic!("no site row anchored at {row_anchor:?}:\n{md}"));
    row.split('|')
        .map(str::trim)
        .nth(idx)
        .unwrap_or_else(|| panic!("row {row:?} has no cell at the disposition index {idx}"))
        .to_owned()
}

/// Every site row carries a disposition RESOLVED FROM THE TABLES.
///
/// PRD §4 D9 requires γ to record each of its choices in the survey artifact.
/// Recording them as hand-written prose would rot the moment a table entry moves
/// or an owning task lands, so the artifact projects the tables instead: the
/// tables are the single source, and this column is a view of them.
///
/// Driven entirely by SYNTHETIC sites, like every other renderer test here — no
/// corpus compile, and no assertion on the committed artifact's own text, which
/// would be a documentation meta-test.
///
/// # Degrades to the table-free rows when a table drains
///
/// The two table-keyed rows borrow the FIRST live entry of each table, so no key
/// is copied here and an individual retirement cannot stale this test. Draining a
/// table EMPTY is the terminal success state of this whole effort, though — the
/// last engineer to delete a residual row must not be greeted by a red gate for
/// having finished the work — so each table-keyed half is skipped with a printed
/// reason, exactly as the `git_is_available` probes in this module skip. The rows
/// that depend on NO table entry (unattributed, unkeyable, `n/a`) always run, so
/// the renderer's projection keeps real coverage after both tables are gone.
#[test]
fn render_survey_resolves_each_site_disposition_from_the_tables() {
    let mut unkeyable = synth_site("no_param.ri", 4, "Widget", "ignored", Owner::Unknown);
    unkeyable.field = None;
    // A ctor-conformance CODE at Error severity: a deliberate rejection fixture,
    // keyable by param yet owned by nobody. Synthetic rather than borrowed from
    // the corpus, and deliberately NOT in either table, so it proves severity
    // alone decides the `n/a` state.
    let mut rejection = synth_site("rejection_fixture.ri", 5, "union", "faces", Owner::NonFea);
    rejection.severity = "Error".to_owned();
    rejection.code = "SelectorKindMismatch".to_owned();

    let mut sites = vec![
        synth_site("not_in_any_table.ri", 3, "Widget", "label", Owner::NonFea),
        unkeyable,
        rejection,
    ];

    let residual = CTOR_CONFORMANCE_CORPUS_RESIDUAL.first();
    let debt = super::examples_smoke::CTOR_CONFORMANCE_MIGRATION_DEBT.first();
    let debt_path = debt.map(|(key, _, _)| format!("{EXAMPLES_PREFIX}{key}"));
    if let Some((path, param, _, _)) = residual {
        sites.push(synth_site(path, 1, "Widget", param, Owner::NonFea));
    }
    if let (Some((_, param, _)), Some(path)) = (debt, debt_path.as_deref()) {
        sites.push(synth_site(path, 2, "TOTSShaper", param, Owner::NonFea));
    }
    let n = sites.len();
    let md = render_survey(
        &SurveyRun {
            total: n,
            surveyed: n,
            not_surveyed: vec![],
            partial: vec![],
            sites,
        },
        &SurveyStamp::at("sha"),
    );

    for (anchor, what) in [
        ("| `not_in_any_table.ri:3`", "a site named by neither table"),
        (
            "| `no_param.ri:4`",
            "a site whose param could not be recovered, so no table can key it",
        ),
    ] {
        let unattributed = disposition_cell(&md, anchor);
        assert!(
            unattributed.contains("unattributed"),
            "{what} must render as unattributed — the conservative default. Reading \
             as deferred would attribute an owner nobody assigned; got \
             {unattributed:?}"
        );
        assert!(
            !unattributed.contains('#'),
            "{what} must name no owner at all; got {unattributed:?}"
        );
    }

    // The third state, and the one the `why` column exists to protect: an
    // Error-severity row must NOT read as unclaimed work.
    let rejection_cell = disposition_cell(&md, "| `rejection_fixture.ri:5`");
    assert!(
        rejection_cell.starts_with("n/a"),
        "an Error-severity ctor-conformance-coded site must render as `n/a` — it is \
         outside the warning signal and nobody owns retiring it; got {rejection_cell:?}"
    );
    assert!(
        !rejection_cell.contains("unattributed") && !rejection_cell.contains('#'),
        "an `n/a` row must neither read as actionable nor name an owner: it is a \
         deliberate rejection fixture, so both would send a reader to delete another \
         PRD's signal; got {rejection_cell:?}"
    );

    // A deferred cell must be a visible deferral, not merely a bare cite.
    if let Some((residual_path, _, residual_owner, residual_why)) = residual {
        let residual_cell = disposition_cell(&md, &format!("| `{residual_path}:1`"));
        assert!(
            residual_cell.contains("deferred")
                && residual_cell.contains(&format!("owned by {residual_owner}")),
            "a deferred site must SAY it is deferred and who owns it, so the cell \
             reads on its own; got {residual_cell:?}"
        );
        assert!(
            residual_cell.contains(&cell(residual_why)),
            "a CTOR_CONFORMANCE_CORPUS_RESIDUAL site must render its recorded reason \
             beside the owner, so a reader meets the deferral and its justification in \
             the same cell; why {residual_why:?}, got {residual_cell:?}"
        );
    } else {
        println!(
            "skipped: CTOR_CONFORMANCE_CORPUS_RESIDUAL is empty — every owner has \
             landed, so there is no residual row to project"
        );
    }

    if let (Some((_, _, debt_owner)), Some(debt_path)) = (debt, debt_path.as_deref()) {
        let debt_cell = disposition_cell(&md, &format!("| `{debt_path}:2`"));
        assert!(
            debt_cell.contains("deferred") && debt_cell.contains(&format!("owned by {debt_owner}")),
            "a CTOR_CONFORMANCE_MIGRATION_DEBT site must render THAT table's owner as a \
             visible deferral. The debt list is keyed relative to `examples/` and this \
             row is repo-relative, so a miss here means the two key forms stopped being \
             bridged and every examples/ site silently reads as unattributed; owner \
             {debt_owner}, got {debt_cell:?}"
        );
    } else {
        println!(
            "skipped: CTOR_CONFORMANCE_MIGRATION_DEBT is empty — every owner has \
             landed, so there is no examples/-keyed row to project"
        );
    }
}

// ─── step 13/14: output path + the generator entry point ─────────────────────

/// Env var that redirects the generator's output to a scratch path.
const OUT_ENV: &str = "REIFY_CTOR_SURVEY_OUT";

/// The committed artifact's repo-relative path.
const ARTIFACT_REL: &str = "docs/prds/struct-ctor-field-type-conformance.survey.md";

/// Resolve the output path from an already-read override value.
///
/// A pure seam so the override can be tested without setting a process-global
/// env var, which would race every other test in this binary. An empty override
/// falls back to the default: an accidental `REIFY_CTOR_SURVEY_OUT=` must not
/// drop the artifact into the current working directory.
fn survey_output_path_for(override_value: Option<String>) -> PathBuf {
    match override_value {
        Some(v) if !v.trim().is_empty() => PathBuf::from(v),
        _ => PathBuf::from(WORKSPACE_ROOT).join(ARTIFACT_REL),
    }
}

/// Where the generator writes: the committed artifact, unless
/// [`OUT_ENV`] redirects it.
///
/// Defaulting to the real location is what lets [`REGEN_COMMAND`] carry no path
/// argument — so the command committed inside the artifact cannot drift from
/// where the artifact actually lives.
fn survey_output_path() -> PathBuf {
    survey_output_path_for(std::env::var(OUT_ENV).ok())
}

/// Run one git command at the workspace root and return its trimmed stdout.
///
/// Every invocation is `-C WORKSPACE_ROOT` (the test process's own CWD is the
/// crate directory, not the repo root), and every one keeps the non-zero-exit
/// assertion: a git read that silently failed would feed an empty string into
/// [`stamp_decision`], which is precisely the "looks clean" reading that must
/// never be reachable by accident.
fn git_read(args: &[&str]) -> String {
    let out = git_at_workspace_root(args)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "ctor_conformance_corpus_survey: cannot run `git {}` in {WORKSPACE_ROOT}: {e}",
                args.join(" ")
            )
        });
    assert!(
        out.status.success(),
        "ctor_conformance_corpus_survey: `git {}` in {WORKSPACE_ROOT} exited {:?}: {}",
        args.join(" "),
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).trim()
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
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
fn survey_stamp() -> SurveyStamp {
    let anchor = git_read(&["merge-base", "main", "HEAD"]);
    // `--untracked-files=no` is deliberate: `git ls-files` never surfaces an
    // untracked file, so an untracked scratch file cannot change one row of the
    // survey and must not trigger a spurious refusal. A staged addition still
    // appears as `A ` and is still caught.
    let dirty = git_read(&["status", "--porcelain", "--untracked-files=no"]);
    let ri_drift = git_read(&["diff", "--name-only", &anchor, "HEAD", "--", "*.ri"]);
    stamp_decision(&anchor, &dirty, &ri_drift)
        .unwrap_or_else(|e| panic!("ctor_conformance_corpus_survey: {e}"))
}

/// **The survey generator.** Sweeps every tracked `.ri` and writes the artifact.
///
/// `#[ignore]`d because it compiles the entire tracked corpus — ~2.5× the
/// `examples/` walk that `examples_smoke.rs` already documents as "the single
/// most expensive thing this binary does". Running it on every merge gate would
/// directly fight `docs/prds/merge-gate-compile-cost.md`.
///
/// The ignore reason is deliberately OPERATIONAL, not blocker-prose: per
/// `docs/prds/reify-audit-ptodo-detector.md` §8 (row 8, the
/// `#[ignore = "requires OCCT"]` class) an operational reason produces no PTODO
/// finding and needs no `#NNNN` cite. One is deliberately NOT written here — a
/// cite would be liveness-checked and would go orphaned the moment task #5304
/// closes.
///
/// Nothing is lost to the ignore: every DECISION this test makes lives in the
/// pure helpers above, each unit-tested on every gate run, plus one cheap
/// three-file end-to-end sweep.
#[test]
#[ignore = "corpus survey generator over every tracked .ri (~660 files and growing); run explicitly with --ignored — see docs/prds/struct-ctor-field-type-conformance.survey.md"]
fn generate_ctor_conformance_corpus_survey() {
    let corpus = tracked_ri_corpus();
    let run = survey_corpus(std::path::Path::new(WORKSPACE_ROOT), corpus);
    let rendered = render_survey(&run, &survey_stamp());
    let out = survey_output_path();
    std::fs::write(&out, &rendered)
        .unwrap_or_else(|e| panic!("cannot write survey to {}: {e}", out.display()));
    println!(
        "ctor-conformance survey: {} sites across {} tracked .ri ({} surveyed, \
         {} not surveyed, {} partial) -> {}",
        run.sites.len(),
        run.total,
        run.surveyed,
        run.not_surveyed.len(),
        run.partial.len(),
        out.display()
    );

    // Asserted AFTER the write, deliberately: a failing run still leaves a
    // regenerated artifact on disk, so the operator can read the disposition
    // column to see which sites the panic is talking about.
    assert_no_unwaived_ctor_conformance_warnings(&run);
}

#[test]
fn survey_output_path_defaults_to_the_committed_artifact_location() {
    // The default must match the artifact's real location exactly, so the
    // regeneration command committed INSIDE the artifact needs no path argument
    // and cannot drift from where the file actually lives.
    //
    // Asserted against the PURE seam `survey_output_path_for(None)`, never
    // against `survey_output_path()`: the latter reads the process-global
    // `REIFY_CTOR_SURVEY_OUT`, which this artifact itself tells operators to
    // export when diffing a fresh run against the committed copy. A
    // gate-resident test that reads it reds for an operator doing exactly what
    // the artifact documents — no defect, pure false alarm.
    let path = survey_output_path_for(None);
    let expected = std::path::Path::new(WORKSPACE_ROOT)
        .join("docs/prds/struct-ctor-field-type-conformance.survey.md");
    assert_eq!(
        path, expected,
        "the default output path must be the committed artifact location"
    );
    let parent = path.parent().expect("the artifact path has a parent");
    assert!(
        parent.is_dir(),
        "the artifact's parent directory {} must exist",
        parent.display()
    );
}

#[test]
fn survey_output_path_honours_the_scratch_override() {
    // The override exists so the sweep can be re-run into a scratch path and
    // diffed against the committed copy WITHOUT dirtying the tree — which is
    // exactly how step 15 proves byte-for-byte reproducibility.
    assert_eq!(
        survey_output_path_for(Some("/tmp/scratch-survey.md".to_owned())),
        std::path::PathBuf::from("/tmp/scratch-survey.md"),
        "REIFY_CTOR_SURVEY_OUT must override the default"
    );
    // Both fallbacks are compared against the DEFAULT PATH ITSELF rather than
    // against `survey_output_path()`, so nothing here depends on the ambient
    // value of `REIFY_CTOR_SURVEY_OUT`.
    let default_path = std::path::Path::new(WORKSPACE_ROOT).join(ARTIFACT_REL);
    assert_eq!(
        survey_output_path_for(None),
        default_path,
        "an unset override must fall back to the committed location"
    );
    assert_eq!(
        survey_output_path_for(Some(String::new())),
        default_path,
        "an EMPTY override must fall back too — an accidental `REIFY_CTOR_SURVEY_OUT=` \
         must not write the artifact to the current directory"
    );
}

// ─── step 17/18: a rebase-durable stamp, refused when it would be dishonest ──

/// A full git object name as `git rev-parse`/`git merge-base` print one:
/// exactly 40 lowercase hexadecimal characters.
const FULL_SHA_LEN: usize = 40;

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
struct SurveyStamp {
    /// The commit the header names — `git merge-base main HEAD`; see
    /// [`survey_stamp`] for why never the branch tip.
    anchor: String,
    /// Tracked `.ri` differing between [`Self::anchor`] and the surveyed
    /// commit, verbatim from `git diff --name-only <anchor> HEAD -- '*.ri'`.
    ///
    /// Empty is the ordinary case and renders NOTHING, so an undrifted artifact
    /// carries no disclosure at all — no permanent "0 files drifted" row, and
    /// runs generated on `main` stay byte-comparable with each other.
    drifted_ri: Vec<String>,
}

impl SurveyStamp {
    /// The undrifted shape: an anchor that describes the surveyed corpus exactly.
    fn at(anchor: &str) -> Self {
        Self {
            anchor: anchor.to_owned(),
            drifted_ri: Vec::new(),
        }
    }
}

/// Decide whether the git state just read may be stamped into the artifact
/// header — or whether stamping it would make that header lie.
///
/// Pure by construction, so the decision is gate-resident and unit-tested with
/// no git state at all; the three reads that feed it live in [`survey_stamp`],
/// behind the `#[ignore]`d generator. Same split this module uses throughout.
///
/// * `anchor` — `git merge-base main HEAD`, the commit the header will name.
/// * `dirty` — `git status --porcelain --untracked-files=no`.
/// * `ri_drift` — `git diff --name-only <anchor> HEAD -- '*.ri'`.
///
/// The first two can REFUSE; `ri_drift` never does — it is disclosed. See
/// [`SurveyStamp`] for the reachability argument that splits them.
///
/// Whitespace-only input is an EMPTY read: git writes a trailing newline even
/// when it has nothing to report.
fn stamp_decision(anchor: &str, dirty: &str, ri_drift: &str) -> Result<SurveyStamp, String> {
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
        drifted_ri: ri_drift
            .lines()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
            .collect(),
    })
}

#[test]
fn stamp_decision_accepts_a_resolved_anchor_over_a_clean_tree() {
    // The one shape that may be stamped: a fully-resolved anchor, nothing
    // uncommitted, and no tracked `.ri` differing between the anchor and the
    // commit actually swept.
    let anchor = "a46387d1f58fb469ed226cc0f2bfbaafa7cf63be";
    assert_eq!(
        stamp_decision(anchor, "", ""),
        Ok(SurveyStamp::at(anchor)),
        "a resolved anchor over a clean, undrifted tree is exactly what the \
         header is allowed to claim"
    );
    // git writes a trailing newline even when it has nothing to report, and a
    // whitespace-only read is an EMPTY read — not a refusal.
    assert_eq!(
        stamp_decision(anchor, "\n", "  \n"),
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
    let err = stamp_decision(anchor, " M crates/reify-compiler/src/lib.rs\n", "")
        .expect_err("a dirty tree must refuse to stamp");
    assert!(
        err.contains("crates/reify-compiler/src/lib.rs"),
        "the refusal must name the dirty path it read, so the operator can act \
         on it without re-running git by hand; got: {err}"
    );
    // A STAGED addition is still dirty — `--untracked-files=no` suppresses the
    // `??` rows only, never the `A `/` M` ones.
    assert!(
        stamp_decision(anchor, "A  docs/prds/new.md\n", "").is_err(),
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
    let stamp = stamp_decision(anchor, "", "examples/one.ri\nexamples/two.ri\n")
        .expect("a committed .ri drift is disclosed, never refused");
    assert_eq!(
        stamp.anchor, anchor,
        "drift must not promote the surveyed tip: the merge base stays THE \
         anchor, because a branch tip is rewritable and this one is not"
    );
    assert_eq!(
        stamp.drifted_ri,
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
    let err = stamp_decision(anchor, " M docs/prds/x.md\n", "examples/one.ri\n")
        .expect_err("a dirty tree refuses whether or not a tracked .ri drifted");
    assert!(
        err.contains("docs/prds/x.md"),
        "the refusal must still name what is dirty; got: {err}"
    );
    assert!(
        stamp_decision("HEAD", "", "examples/one.ri\n").is_err(),
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
            stamp_decision(anchor, "", "").is_err(),
            "{anchor:?} must be rejected rather than stamped: {why}"
        );
    }
    // Sanity: the guard rejects for the RIGHT reason — the same inputs with a
    // well-formed anchor are accepted.
    assert!(
        stamp_decision(&"a".repeat(FULL_SHA_LEN), "", "").is_ok(),
        "40 lowercase hex characters is the accepted shape"
    );
}

// ─── step 19/20: the committed stamp must stay reachable ─────────────────────

/// The `<sha>` from the artifact's ``**Base commit:** `<sha>` `` header line.
///
/// `None` when the line is absent or does not have that exact shape — the
/// caller treats that as a FAILURE, never as a skip, so a renderer change
/// cannot silently defeat the parse and take the guard with it.
fn parse_stamped_base_commit(md: &str) -> Option<&str> {
    md.lines()
        .filter_map(|line| line.trim_end().strip_prefix("**Base commit:** `"))
        .find_map(|rest| rest.strip_suffix('`'))
}

/// Run one git command at the workspace root and report only whether it
/// SUCCEEDED.
///
/// For the git PREDICATES — `cat-file -e`, `merge-base --is-ancestor` — whose
/// entire answer is the exit code, and where a non-zero exit is the finding
/// rather than an infrastructure failure. [`git_read`] would panic on it.
fn git_succeeds(args: &[&str]) -> bool {
    git_at_workspace_root(args)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "ctor_conformance_corpus_survey: cannot run `git {}` in {WORKSPACE_ROOT}: {e}",
                args.join(" ")
            )
        })
        .status
        .success()
}

#[test]
fn committed_survey_stamps_a_commit_that_is_an_ancestor_of_head() {
    // Makes the dangling-anchor defect class LOUD on every gate run instead of
    // leaving it for a human reviewer to catch. Cost is one file read plus
    // three git calls — so unlike the `#[ignore]`d generator this belongs here.
    //
    // ORDERING DEPENDENCY: this guard stays green across future rebases ONLY
    // because `survey_stamp()` stamps `git merge-base main HEAD` rather than the
    // branch tip. Adding it while the anchor was still a branch tip would
    // convert every rebase of this branch into a merge-blocking red.
    //
    // Unlike `survey_stamp()`, nothing here needs a `main` ref to exist.
    //
    // SKIP, not fail, when git cannot be spawned: every assertion below is a
    // question put to git, and "there is no git" is not an answer about the
    // artifact. See `git_is_available` for why this module must not be what
    // makes git a hard requirement of the reify-compiler suite.
    if !git_is_available() {
        println!("skipped: no `git` on PATH — see `git_is_available`");
        return;
    }

    // SKIP, not fail, when the artifact is ABSENT.
    //
    // This module's header states the artifact is a point-in-time snapshot, not
    // a freshness-gated golden file, and γ (task #5305) will legitimately
    // invalidate it. The natural end of life for a consumed census is DELETION —
    // see "Retiring this module" in the header. Panicking on absence would make
    // that one-file deletion red the whole `reify-compiler` merge gate with a
    // message that reads like a compiler defect, and whoever deletes a document
    // under `docs/prds/` has no reason to look inside a compiler test binary for
    // the cause.
    //
    // What is NOT relaxed: if the file IS present, every assertion below is
    // hard. An artifact that exists while naming an unresolvable or orphaned
    // anchor is a real defect, and that is the case this guard was written for.
    let artifact = std::path::Path::new(WORKSPACE_ROOT).join(ARTIFACT_REL);
    if !artifact.is_file() {
        println!(
            "skipped: {ARTIFACT_REL} is not present — the survey snapshot has been \
             retired or not yet generated; nothing to check"
        );
        return;
    }
    let md = std::fs::read_to_string(&artifact).unwrap_or_else(|e| {
        // Present but unreadable is an I/O failure, not a retirement.
        panic!(
            "cannot read the committed survey at {}: {e}",
            artifact.display()
        )
    });

    // (a) The header line must be present AND parseable. A missing or
    //     reshaped line FAILS rather than skipping: a renderer change must not
    //     be able to defeat the parse and silently disarm (b) and (c) with it.
    let sha = parse_stamped_base_commit(&md).unwrap_or_else(|| {
        panic!(
            "{} must carry a `**Base commit:** `<sha>`` header line — without it \
             the artifact's snapshot claim names nothing checkable",
            ARTIFACT_REL
        )
    });
    assert!(
        sha.len() == FULL_SHA_LEN && sha.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
        "the stamped base commit must be {FULL_SHA_LEN} lowercase hex characters, \
         got {sha:?}"
    );

    // The one environment that could produce a FALSE red: in a shallow clone
    // the anchor object may legitimately be absent, and a false red here would
    // deadlock the merge queue. Verified `false` in this worktree, so the
    // assertions below really do run.
    if git_read(&["rev-parse", "--is-shallow-repository"]) == "true" {
        return;
    }

    // (b) The object actually exists in this repository. This alone catches a
    //     stamp that survives only as a dangling object in one worktree.
    assert!(
        git_succeeds(&["cat-file", "-e", &format!("{sha}^{{commit}}")]),
        "the stamped base commit {sha} does not exist as a commit in this \
         repository — the artifact header names an unresolvable object"
    );

    // (c) It is reachable from the tip, so it survives the `--no-ff` merge onto
    //     main. A rebase orphaning the stamped commit reds exactly here.
    assert!(
        git_succeeds(&["merge-base", "--is-ancestor", sha, "HEAD"]),
        "the stamped base commit {sha} is not an ancestor of HEAD — it was \
         orphaned (a rebase, most likely) and will vanish when this branch \
         lands. Re-run the survey generator to re-stamp the merge base."
    );
}
